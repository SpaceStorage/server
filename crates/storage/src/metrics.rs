//! Additive `db_wal_*` metrics hooks (013 T011 / T060). Exposition via 008.

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
pub struct WalMetrics {
    pub wal_bytes_total: AtomicU64,
    pub wal_fsync_total: AtomicU64,
    pub wal_fsync_duration_ns: AtomicU64,
    pub wal_torn_total: AtomicU64,
    pub wal_disk_full_total: AtomicU64,
    pub wal_replay_duration_ns: AtomicU64,
    pub wal_recovery_duration_ns: AtomicU64,
    pub recovery_total: AtomicU64,
    pub recovery_duration_ns: AtomicU64,
    pub recovery_records_total: AtomicU64,
    pub unflushed_bytes: AtomicU64,
    pub wal_lag_bytes: AtomicU64,
}

impl WalMetrics {
    pub fn record_fsync(&self, bytes: u64, duration: std::time::Duration) {
        self.wal_bytes_total.fetch_add(bytes, Ordering::Relaxed);
        self.wal_fsync_total.fetch_add(1, Ordering::Relaxed);
        self.wal_fsync_duration_ns
            .fetch_add(duration.as_nanos() as u64, Ordering::Relaxed);
    }

    /// Prometheus text fragment for reserved `db_wal_*` series.
    pub fn render_prometheus(&self, node: &str, drive: &str) -> String {
        format!(
            "# HELP db_wal_bytes_total Appended WAL bytes.\n\
             # TYPE db_wal_bytes_total counter\n\
             db_wal_bytes_total{{node=\"{node}\",drive=\"{drive}\"}} {}\n\
             # HELP db_wal_fsync_total Group-commit fsyncs.\n\
             # TYPE db_wal_fsync_total counter\n\
             db_wal_fsync_total{{node=\"{node}\",drive=\"{drive}\"}} {}\n\
             # HELP db_wal_torn_total Torn WAL tails skipped.\n\
             # TYPE db_wal_torn_total counter\n\
             db_wal_torn_total{{node=\"{node}\",drive=\"{drive}\"}} {}\n\
             # HELP db_wal_disk_full_total Disk-full events.\n\
             # TYPE db_wal_disk_full_total counter\n\
             db_wal_disk_full_total{{node=\"{node}\",drive=\"{drive}\"}} {}\n\
             # HELP db_recovery_total Recovery attempts.\n\
             # TYPE db_recovery_total counter\n\
             db_recovery_total{{node=\"{node}\",result=\"ok\"}} {}\n\
             # HELP db_recovery_records_total Replayed records.\n\
             # TYPE db_recovery_records_total counter\n\
             db_recovery_records_total{{node=\"{node}\",drive=\"{drive}\"}} {}\n\
             # HELP db_unflushed_bytes Unflushed WAL bytes.\n\
             # TYPE db_unflushed_bytes gauge\n\
             db_unflushed_bytes{{node=\"{node}\",drive=\"{drive}\"}} {}\n\
             # HELP db_wal_lag_bytes WAL lag bytes.\n\
             # TYPE db_wal_lag_bytes gauge\n\
             db_wal_lag_bytes{{node=\"{node}\",drive=\"{drive}\"}} {}\n",
            self.wal_bytes_total.load(Ordering::Relaxed),
            self.wal_fsync_total.load(Ordering::Relaxed),
            self.wal_torn_total.load(Ordering::Relaxed),
            self.wal_disk_full_total.load(Ordering::Relaxed),
            self.recovery_total.load(Ordering::Relaxed),
            self.recovery_records_total.load(Ordering::Relaxed),
            self.unflushed_bytes.load(Ordering::Relaxed),
            self.wal_lag_bytes.load(Ordering::Relaxed),
        )
    }
}
