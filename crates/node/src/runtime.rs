use std::thread::available_parallelism;
use tracing::warn;

pub fn resolve_worker_threads(configured: Option<u32>) -> (u32, String) {
    if let Some(n) = configured {
        return (n.max(1), "configured".into());
    }
    match available_parallelism() {
        Ok(n) => (n.get() as u32, "available_cores".into()),
        Err(e) => {
            warn!(error = %e, "available_parallelism failed; falling back to 1 worker thread");
            (1, "fallback".into())
        }
    }
}

pub fn build_runtime(worker_threads: u32) -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(worker_threads.max(1) as usize)
        .enable_all()
        .thread_name("ss-worker")
        .build()
        .expect("tokio runtime")
}
