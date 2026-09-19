use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

pub struct Stats {
    worker_threads: AtomicU32,
    worker_threads_busy: AtomicU32,
    drain_timed_out: AtomicBool,
}

impl Stats {
    pub fn new(workers: u32) -> Self {
        Self {
            worker_threads: AtomicU32::new(workers),
            worker_threads_busy: AtomicU32::new(0),
            drain_timed_out: AtomicBool::new(false),
        }
    }

    pub fn busy(&self) -> u32 {
        self.worker_threads_busy.load(Ordering::Relaxed)
    }

    pub fn set_busy(&self, n: u32) {
        self.worker_threads_busy.store(n, Ordering::Relaxed);
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
