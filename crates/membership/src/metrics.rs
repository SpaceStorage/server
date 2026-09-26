use std::sync::atomic::{AtomicU64, Ordering};

/// Closed metric names from 011 contracts/metrics.md (stub increments).
#[derive(Debug, Default)]
pub struct MembershipMetrics {
    pub members: AtomicU64,
    pub pending: AtomicU64,
    pub join_ok: AtomicU64,
    pub join_refused: AtomicU64,
    pub replace_ok: AtomicU64,
    pub replace_refused: AtomicU64,
    pub decommission_ok: AtomicU64,
    pub decommission_refused: AtomicU64,
    pub secret_epoch: AtomicU64,
}

impl MembershipMetrics {
    pub fn set_members(&self, n: u64) {
        self.members.store(n, Ordering::Relaxed);
    }
    pub fn set_pending(&self, n: u64) {
        self.pending.store(n, Ordering::Relaxed);
    }
    pub fn join_ok(&self) {
        self.join_ok.fetch_add(1, Ordering::Relaxed);
    }
    pub fn join_refused(&self) {
        self.join_refused.fetch_add(1, Ordering::Relaxed);
    }
    pub fn replace_ok(&self) {
        self.replace_ok.fetch_add(1, Ordering::Relaxed);
    }
    pub fn replace_refused(&self) {
        self.replace_refused.fetch_add(1, Ordering::Relaxed);
    }
    pub fn decommission_ok(&self) {
        self.decommission_ok.fetch_add(1, Ordering::Relaxed);
    }
    pub fn decommission_refused(&self) {
        self.decommission_refused.fetch_add(1, Ordering::Relaxed);
    }
    pub fn set_secret_epoch(&self, epoch: u64) {
        self.secret_epoch.store(epoch, Ordering::Relaxed);
    }
}
