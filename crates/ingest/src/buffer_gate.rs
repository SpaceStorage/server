//! Shared overflow gate so node `BufferRegistry` can drive ingest reject/drop.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Reserve/release for decode/recv paths. Reject when full — never wait.
pub trait BufferGate: Send + Sync {
    fn try_reserve(&self, n: u64) -> bool;
    fn release(&self, n: u64);
    fn capacity(&self) -> u64;
}

/// Local reject buffer (unit tests / fallback when registry not wired).
#[derive(Debug)]
pub struct LocalBufferGate {
    capacity: AtomicU64,
    used: AtomicU64,
    limit_hits: AtomicU64,
}

impl LocalBufferGate {
    pub fn new(capacity: u64) -> Self {
        Self {
            capacity: AtomicU64::new(capacity),
            used: AtomicU64::new(0),
            limit_hits: AtomicU64::new(0),
        }
    }

    pub fn limit_hits(&self) -> u64 {
        self.limit_hits.load(Ordering::SeqCst)
    }

    pub fn used(&self) -> u64 {
        self.used.load(Ordering::SeqCst)
    }
}

impl BufferGate for LocalBufferGate {
    fn try_reserve(&self, n: u64) -> bool {
        loop {
            let used = self.used.load(Ordering::SeqCst);
            let cap = self.capacity.load(Ordering::SeqCst);
            if used + n > cap {
                self.limit_hits.fetch_add(1, Ordering::SeqCst);
                return false;
            }
            if self
                .used
                .compare_exchange(used, used + n, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return true;
            }
        }
    }

    fn release(&self, n: u64) {
        let cur = self.used.load(Ordering::SeqCst);
        self.used.store(cur.saturating_sub(n), Ordering::SeqCst);
    }

    fn capacity(&self) -> u64 {
        self.capacity.load(Ordering::SeqCst)
    }
}

pub type SharedBufferGate = Arc<dyn BufferGate>;
