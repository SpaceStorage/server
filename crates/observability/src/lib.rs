//! Global /metrics exposition (008 US1).
//! Reserved `db_wal_*` series are owned/incremented by `spacestorage-storage`
//! (013 T060 — provisional `spacestorage_wal_acks_total` retired).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

pub struct Metrics {
    pub started: Instant,
    pub queries_total: AtomicU64,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            started: Instant::now(),
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
             # HELP spacestorage_queries_total Queries executed.\n\
             # TYPE spacestorage_queries_total counter\n\
             spacestorage_queries_total {}\n",
            self.queries_total.load(Ordering::Relaxed),
        )
    }
}
