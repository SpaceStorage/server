use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// In-memory gauges/counters with Prometheus-conformant reserved names (feature 08).
pub struct Stats {
    worker_threads: AtomicU32,
    /// Approximate busy workers via in-flight handler tasks (T035 / T080 fallback).
    worker_threads_busy: AtomicU32,
    drain_timed_out: AtomicBool,
    inflight: AtomicU32,
}

impl Stats {
    pub fn new(workers: u32) -> Self {
        Self {
            worker_threads: AtomicU32::new(workers),
            worker_threads_busy: AtomicU32::new(0),
            drain_timed_out: AtomicBool::new(false),
            inflight: AtomicU32::new(0),
        }
    }

    pub fn busy(&self) -> u32 {
        self.worker_threads_busy.load(Ordering::Relaxed)
    }

    pub fn set_busy(&self, n: u32) {
        self.worker_threads_busy.store(n, Ordering::Relaxed);
    }

    pub fn inc_inflight(&self) {
        let n = self.inflight.fetch_add(1, Ordering::Relaxed) + 1;
        let workers = self.worker_threads.load(Ordering::Relaxed).max(1);
        self.worker_threads_busy
            .store(n.min(workers), Ordering::Relaxed);
    }

    pub fn dec_inflight(&self) {
        let prev = self.inflight.load(Ordering::Relaxed);
        let n = if prev == 0 {
            0
        } else {
            self.inflight.fetch_sub(1, Ordering::Relaxed) - 1
        };
        let workers = self.worker_threads.load(Ordering::Relaxed).max(1);
        self.worker_threads_busy
            .store(n.min(workers), Ordering::Relaxed);
    }

    pub fn inflight(&self) -> u32 {
        self.inflight.load(Ordering::Relaxed)
    }

    pub fn mark_drain_timed_out(&self) {
        self.drain_timed_out.store(true, Ordering::SeqCst);
    }

    pub fn drain_timed_out(&self) -> bool {
        self.drain_timed_out.load(Ordering::SeqCst)
    }

    pub fn workers(&self) -> u32 {
        self.worker_threads.load(Ordering::Relaxed)
    }
}
