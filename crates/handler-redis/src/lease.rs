//! Total-order stream leadership lease (`017` T038, research R6, constitution XII).
//!
//! A stream key whose order is `Total` must serialize appends through one writer
//! and fence each append with a current epoch; the handler crate MUST NOT depend
//! on `spacestorage-controlplane` (`006`), so the epoch fence is injected through
//! the [`StreamLeaseProvider`] seam. A running node injects a provider backed by
//! the real `crates/controlplane/src/lease.rs` primitives (see
//! `crates/node/src/handler/redis.rs`); the constructors here install
//! [`StreamLeaseAuthority::single_node`] as the **dev / harness fallback** so an
//! in-process test or single-node bring-up still has a sole writer.
//!
//! The fence mirrors `controlplane::lease`: `grant` yields the current epoch (or
//! `NotGranted` when this process is not the holder and auto-grant is off), and
//! `check` refuses a stale/unknown epoch without storing anything. It never
//! "falls open": a stream with no holder cannot be written.

use std::collections::BTreeMap;
use std::sync::Mutex;

pub use spacestorage_protocol_core::lease::{StreamLeaseError, StreamLeaseProvider};
/// Back-compat name: the handler's historical `LeaseError` is the shared seam error.
pub use spacestorage_protocol_core::lease::StreamLeaseError as LeaseError;

/// Per-stream-key leadership lease registry **for dev / tests / single-node
/// bring-up**. Shared across every connection of a running node; a real
/// deployment substitutes a `controlplane`-backed [`StreamLeaseProvider`].
#[derive(Debug)]
pub struct StreamLeaseAuthority {
    holder: u64,
    auto_grant: bool,
    epochs: Mutex<BTreeMap<String, u64>>,
}

impl StreamLeaseAuthority {
    /// Single-node deployment: this process is the holder and auto-grants epoch 1
    /// on first use, so a stock `XADD` (the default `Total` order) succeeds.
    pub fn single_node() -> Self {
        Self {
            holder: 1,
            auto_grant: true,
            epochs: Mutex::new(BTreeMap::new()),
        }
    }

    /// A holder that refuses to take an unheld lease (used to pin the "no lease
    /// holder → write nothing, do not fall open to producer order" invariant).
    pub fn non_granting(holder: u64) -> Self {
        Self {
            holder,
            auto_grant: false,
            epochs: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn holder(&self) -> u64 {
        self.holder
    }

    /// Hand the lease to a new epoch (a `006` steal) — the previous holder's
    /// appends become stale. Used to exercise the epoch fence.
    pub fn steal(&self, key: &str) -> u64 {
        let mut m = self.epochs.lock().expect("lease");
        let e = m.entry(key.to_string()).or_insert(1);
        *e = e.saturating_add(1);
        *e
    }
}

impl StreamLeaseProvider for StreamLeaseAuthority {
    /// Request the current epoch for a stream key. Auto-grants epoch 1 when the
    /// process is the single writer and the key has no lease yet; otherwise the
    /// caller is not the holder and the write must not proceed.
    fn grant(&self, key: &str) -> Result<u64, StreamLeaseError> {
        let mut m = self.epochs.lock().expect("lease");
        match m.get(key) {
            Some(e) => Ok(*e),
            None if self.auto_grant => {
                m.insert(key.to_string(), 1);
                Ok(1)
            }
            None => Err(StreamLeaseError::NotGranted),
        }
    }

    /// Fence one total-order append: the presented epoch must equal the current
    /// lease epoch. No holder and no auto-grant → the write is refused.
    fn check(&self, key: &str, presented: u64) -> Result<(), StreamLeaseError> {
        let m = self.epochs.lock().expect("lease");
        match m.get(key) {
            Some(cur) if *cur == presented => Ok(()),
            Some(cur) => Err(StreamLeaseError::Stale {
                have: presented,
                need: *cur,
            }),
            None if self.auto_grant => Ok(()),
            None => Err(StreamLeaseError::NotGranted),
        }
    }

    /// Record the epoch a stream key holds (used on restore so a persisted stream
    /// can continue appending under its persisted lease).
    fn adopt(&self, key: &str, epoch: u64) {
        self.epochs.lock().expect("lease").insert(key.to_string(), epoch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_node_auto_grants_and_fences_stale_epoch() {
        let a = StreamLeaseAuthority::single_node();
        assert_eq!(a.grant("s").unwrap(), 1);
        assert!(a.check("s", 1).is_ok());
        assert_eq!(a.steal("s"), 2);
        assert_eq!(
            a.check("s", 1),
            Err(StreamLeaseError::Stale { have: 1, need: 2 })
        );
        assert!(a.check("s", 2).is_ok());
    }

    #[test]
    fn non_granting_refuses_unheld() {
        let a = StreamLeaseAuthority::non_granting(7);
        assert_eq!(a.grant("s"), Err(StreamLeaseError::NotGranted));
        assert_eq!(a.check("s", 1), Err(StreamLeaseError::NotGranted));
    }

    #[test]
    fn adopt_restores_persisted_epoch() {
        let a = StreamLeaseAuthority::non_granting(7);
        a.adopt("s", 9);
        assert_eq!(a.grant("s"), Ok(9));
        assert!(a.check("s", 9).is_ok());
    }
}
