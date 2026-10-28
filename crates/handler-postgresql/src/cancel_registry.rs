//! Process-wide CancelRequest registry (PostgreSQL BackendKeyData → CancelToken).
//!
//! CancelRequest arrives on a separate TCP connection; the in-flight query's
//! [`CancelToken`] is looked up by `(pid, secret)` and cancelled so work stops
//! inside the timeout window (FR-009).

use spacestorage_query::CancelToken;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Mutex, OnceLock};

static NEXT_PID: AtomicI32 = AtomicI32::new(1000);
static REGISTRY: OnceLock<Mutex<HashMap<(i32, i32), CancelToken>>> = OnceLock::new();

fn map() -> &'static Mutex<HashMap<(i32, i32), CancelToken>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Allocate a unique BackendKeyData pair and register `token` for CancelRequest.
pub fn register(token: CancelToken) -> (i32, i32) {
    let pid = NEXT_PID.fetch_add(1, Ordering::Relaxed);
    let key = pid
        .wrapping_mul(0x9e37_79b1u32 as i32)
        .wrapping_add(0x85eb_ca6bu32 as i32);
    map()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert((pid, key), token);
    (pid, key)
}

pub fn unregister(pid: i32, key: i32) {
    map()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&(pid, key));
}

/// Cancel the matching in-flight exec token. Returns true if a session was found.
pub fn cancel(pid: i32, key: i32) -> bool {
    let g = map().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(tok) = g.get(&(pid, key)) {
        tok.cancel();
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_stops_registered_token() {
        let tok = CancelToken::new();
        let (pid, key) = register(tok.clone());
        assert!(!tok.is_cancelled());
        assert!(cancel(pid, key));
        assert!(tok.is_cancelled());
        unregister(pid, key);
        assert!(!cancel(pid, key));
    }
}
