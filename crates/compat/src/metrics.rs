//! Compat MUST NOT counters (`spacestorage_compat_must_not_total{protocol,verb}`).
//!
//! Labels match [contracts/metrics.md](../../specs/015-compatibility-and-limits/contracts/metrics.md).
//! Process-local; lost on restart. Handlers call [`record_must_not`] on MustNot paths.

use std::collections::HashMap;
use std::sync::Mutex;

static MUST_NOT: Mutex<Option<HashMap<(String, String), u64>>> = Mutex::new(None);

fn with_map<R>(f: impl FnOnce(&mut HashMap<(String, String), u64>) -> R) -> R {
    let mut guard = MUST_NOT.lock().expect("compat must_not metrics");
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    f(guard.as_mut().expect("initialized"))
}

/// Increment `spacestorage_compat_must_not_total{protocol,verb}`.
pub fn record_must_not(protocol: &str, verb: &str) {
    with_map(|m| {
        *m.entry((protocol.to_string(), verb.to_string()))
            .or_insert(0) += 1;
    });
}

/// Read the counter for tests / `/metrics` exporters.
pub fn must_not_count(protocol: &str, verb: &str) -> u64 {
    with_map(|m| m.get(&(protocol.to_string(), verb.to_string())).copied().unwrap_or(0))
}

/// Snapshot all labeled counters.
pub fn must_not_snapshot() -> Vec<(String, String, u64)> {
    with_map(|m| {
        let mut out: Vec<_> = m
            .iter()
            .map(|((p, v), c)| (p.clone(), v.clone(), *c))
            .collect();
        out.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn increments() {
        let before = must_not_count("test-proto", "TEST_VERB");
        record_must_not("test-proto", "TEST_VERB");
        assert_eq!(must_not_count("test-proto", "TEST_VERB"), before + 1);
    }
}
