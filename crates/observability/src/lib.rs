//! Global /metrics exposition (008 US1).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

pub struct Metrics {
    pub started: Instant,
    pub wal_acks_total: AtomicU64,
    pub queries_total: AtomicU64,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            wal_acks_total: AtomicU64::new(0),
            queries_total: AtomicU64::new(0),
        }
    }
}

impl Metrics {
    pub fn render_prometheus(&self) -> String {
        let up = self.started.elapsed().as_secs_f64();
        format!(
            "# HELP spacestorage_process_uptime_seconds Process uptime.\n\
             # TYPE spacestorage_process_uptime_seconds gauge\n\
             spacestorage_process_uptime_seconds {up}\n\
             # HELP spacestorage_wal_acks_total Durable WAL acknowledgements.\n\
             # TYPE spacestorage_wal_acks_total counter\n\
             spacestorage_wal_acks_total {}\n\
             # HELP spacestorage_queries_total Queries executed.\n\
             # TYPE spacestorage_queries_total counter\n\
             spacestorage_queries_total {}\n",
            self.wal_acks_total.load(Ordering::Relaxed),
            self.queries_total.load(Ordering::Relaxed),
        )
    }
}
