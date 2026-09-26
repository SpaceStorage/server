//! Metric catalog: HELP/TYPE families, bucket constants, owner→family map (008).
//!
//! Owner → family (contracts/metrics.md):
//! - `001` node — uptime, node_ready/state, workers, buffers, entrypoint connections
//! - `002` handlers — comm_*, api_*
//! - `004`/`012` — replication_*
//! - `005` exec — query_*
//! - `006` controlplane — raft_leader_election_*
//! - `007` tenancy — namespace_usage_*, quota_exceeded_total (billing labels)
//! - `013` durability — db_wal_*, db_checkpoint_*, db_dirty_*, db_unflushed_*, db_recovery_*
//! - types/LSM/HNSW — db_lsm_*, db_hnsw_*
//! - job owners — spacestorage_job_*
//! - `008` — freshness, export errors, DNS for exporters
//!
//! Missing families are **absent** until owners record them (not zero-filled).

use crate::sample::SampleKind;

/// Duration histogram buckets (seconds) — R10 / catalog.md.
pub const DURATION_BUCKETS: &[f64] = &[
    0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0,
];

/// Size histogram buckets (bytes) — R10 / catalog.md.
pub const SIZE_BUCKETS: &[f64] = &[
    64.0,
    256.0,
    1024.0,
    4096.0,
    16384.0,
    65536.0,
    262144.0,
    1_048_576.0,
    4_194_304.0,
    16_777_216.0,
    67_108_864.0,
];

#[derive(Debug, Clone, Copy)]
pub struct FamilyMeta {
    pub name: &'static str,
    pub kind: SampleKind,
    pub help: &'static str,
}

/// Background job names that MUST be coverable (FR-012).
pub const JOB_NAMES: &[&str] = &[
    "compaction",
    "flush",
    "checkpoint",
    "vacuum",
    "gc",
    "index_rebuild",
    "replication",
    "backup",
    "cleanup",
    "ttl_expiration",
    "snapshot",
    "data_transformation",
    "data_migration",
    "data_backup",
    "data_restore",
];

macro_rules! fam {
    ($name:expr, $kind:ident, $help:expr) => {
        FamilyMeta {
            name: $name,
            kind: SampleKind::$kind,
            help: $help,
        }
    };
}

/// System / `001` reserved series.
pub const SYSTEM_FAMILIES: &[FamilyMeta] = &[
    fam!(
        "spacestorage_uptime_seconds",
        Gauge,
        "Process uptime in seconds."
    ),
    fam!("node_ready", Gauge, "1 when node is ready."),
    fam!(
        "spacestorage_node_ready",
        Gauge,
        "Alias of node_ready (001)."
    ),
    fam!(
        "node_state",
        Gauge,
        "Node state enum: starting=0 ready=1 draining=2 degraded=3 recovering=4 failed=5."
    ),
    fam!(
        "spacestorage_node_state",
        Gauge,
        "Alias of node_state (001)."
    ),
    fam!(
        "spacestorage_worker_threads",
        Gauge,
        "Configured worker thread count."
    ),
    fam!(
        "spacestorage_worker_threads_busy",
        Gauge,
        "Busy worker thread count."
    ),
    fam!(
        "spacestorage_worker_threads_usage_ratio",
        Gauge,
        "Worker usage as 0–1 ratio."
    ),
    fam!(
        "spacestorage_buffer_usage_bytes",
        Gauge,
        "Buffer usage in bytes."
    ),
    fam!(
        "spacestorage_buffer_usage_ratio",
        Gauge,
        "Buffer usage ratio 0–1+."
    ),
    fam!(
        "spacestorage_buffer_capacity_bytes",
        Gauge,
        "Buffer capacity in bytes."
    ),
    fam!(
        "spacestorage_buffer_limit_hits_total",
        Counter,
        "Buffer capacity limit hits."
    ),
    fam!(
        "spacestorage_cache_hits_total",
        Counter,
        "Cache hits."
    ),
    fam!(
        "spacestorage_cache_misses_total",
        Counter,
        "Cache misses."
    ),
    fam!(
        "spacestorage_cache_evictions_total",
        Counter,
        "Cache evictions."
    ),
    fam!(
        "spacestorage_storage_read_duration_seconds",
        Histogram,
        "Storage read duration."
    ),
    fam!(
        "spacestorage_storage_write_duration_seconds",
        Histogram,
        "Storage write duration."
    ),
    fam!(
        "spacestorage_storage_fsync_duration_seconds",
        Histogram,
        "Storage fsync duration."
    ),
    fam!(
        "spacestorage_storage_flush_duration_seconds",
        Histogram,
        "Storage flush duration."
    ),
    fam!(
        "spacestorage_storage_read_bytes_total",
        Counter,
        "Storage bytes read."
    ),
    fam!(
        "spacestorage_storage_write_bytes_total",
        Counter,
        "Storage bytes written."
    ),
    fam!(
        "spacestorage_storage_fsync_bytes_total",
        Counter,
        "Storage fsync bytes."
    ),
    fam!(
        "spacestorage_storage_flush_bytes_total",
        Counter,
        "Storage flush bytes."
    ),
    fam!(
        "spacestorage_dns_resolution_duration_seconds",
        Histogram,
        "DNS resolution duration."
    ),
    fam!(
        "spacestorage_dns_requests_total",
        Counter,
        "DNS requests."
    ),
    fam!(
        "spacestorage_dns_errors_total",
        Counter,
        "DNS errors."
    ),
    fam!(
        "spacestorage_raft_leader_elections_total",
        Counter,
        "Raft leader elections."
    ),
    fam!(
        "spacestorage_raft_leader_election_errors_total",
        Counter,
        "Raft leader election errors."
    ),
    fam!(
        "spacestorage_raft_leader_election_duration_seconds",
        Histogram,
        "Raft leader election duration."
    ),
    fam!(
        "spacestorage_entrypoint_connections_active",
        Gauge,
        "Active connections per entrypoint."
    ),
    fam!(
        "spacestorage_shared_aggregation_up",
        Gauge,
        "1 when shared-datatype aggregation is fresh."
    ),
    fam!(
        "spacestorage_shared_aggregation_last_success_timestamp_seconds",
        Gauge,
        "Unix seconds of last successful shared aggregation."
    ),
    fam!(
        "spacestorage_log_export_errors_total",
        Counter,
        "Log export errors."
    ),
    fam!(
        "spacestorage_otel_export_errors_total",
        Counter,
        "OTLP export errors."
    ),
];

/// Query-processing (FR-006) — owner `exec` / `005`.
pub const QUERY_FAMILIES: &[FamilyMeta] = &[
    fam!(
        "spacestorage_query_blocks_read_total",
        Counter,
        "Query blocks read."
    ),
    fam!(
        "spacestorage_query_blocks_written_total",
        Counter,
        "Query blocks written."
    ),
    fam!(
        "spacestorage_query_bytes_read_total",
        Counter,
        "Query bytes read."
    ),
    fam!(
        "spacestorage_query_bytes_written_total",
        Counter,
        "Query bytes written."
    ),
    fam!("spacestorage_query_total", Counter, "Query totals."),
    fam!(
        "spacestorage_query_errors_total",
        Counter,
        "Query errors."
    ),
    fam!(
        "spacestorage_query_rows_returned_total",
        Counter,
        "Rows/documents/values returned."
    ),
    fam!("spacestorage_query_hits_total", Counter, "Query hits."),
    fam!(
        "spacestorage_query_misses_total",
        Counter,
        "Query misses."
    ),
    fam!(
        "spacestorage_query_lookup_probes_total",
        Counter,
        "Lookup probes."
    ),
    fam!(
        "spacestorage_query_index_scans_total",
        Counter,
        "Index scans."
    ),
    fam!(
        "spacestorage_query_full_scans_total",
        Counter,
        "Full scans."
    ),
    fam!(
        "spacestorage_query_duration_seconds",
        Histogram,
        "Query execution duration."
    ),
    fam!(
        "spacestorage_query_queue_duration_seconds",
        Histogram,
        "Query queue duration."
    ),
    fam!(
        "spacestorage_query_wait_node_duration_seconds",
        Histogram,
        "Query wait-for-node duration."
    ),
    fam!(
        "spacestorage_query_result_size_bytes",
        Histogram,
        "Query result size."
    ),
    fam!(
        "spacestorage_query_retries_total",
        Counter,
        "Query retries."
    ),
    fam!(
        "spacestorage_query_in_flight",
        Gauge,
        "Queries currently in flight."
    ),
];

/// Communication (FR-007).
pub const COMM_FAMILIES: &[FamilyMeta] = &[
    fam!(
        "spacestorage_comm_query_total",
        Counter,
        "Communication query totals."
    ),
    fam!(
        "spacestorage_comm_query_errors_total",
        Counter,
        "Communication query errors."
    ),
    fam!(
        "spacestorage_comm_status",
        Gauge,
        "1 success / 0 error on in-flight communication."
    ),
    fam!(
        "spacestorage_comm_read_duration_seconds",
        Histogram,
        "read() duration."
    ),
    fam!(
        "spacestorage_comm_write_duration_seconds",
        Histogram,
        "write() duration."
    ),
    fam!(
        "spacestorage_comm_connect_attempts_total",
        Counter,
        "Connection attempts."
    ),
    fam!(
        "spacestorage_comm_connect_errors_total",
        Counter,
        "Connection errors."
    ),
    fam!(
        "spacestorage_comm_connections",
        Gauge,
        "Actual connections."
    ),
];

/// Replication (FR-008).
pub const REPLICATION_FAMILIES: &[FamilyMeta] = &[
    fam!(
        "spacestorage_replication_status",
        Gauge,
        "1 healthy / 0 failed."
    ),
    fam!(
        "spacestorage_replication_lag_seconds",
        Gauge,
        "Replication lag seconds."
    ),
    fam!(
        "spacestorage_replication_last_block",
        Gauge,
        "Last replicated block when known."
    ),
    fam!(
        "spacestorage_replication_queue_blocks",
        Gauge,
        "Replication queue blocks."
    ),
    fam!(
        "spacestorage_replication_queue_bytes",
        Gauge,
        "Replication queue bytes."
    ),
    fam!(
        "spacestorage_replication_queue_objects",
        Gauge,
        "Replication queue objects."
    ),
    fam!(
        "spacestorage_replication_objects_waiting",
        Gauge,
        "Objects waiting for replication."
    ),
    fam!(
        "spacestorage_replication_wait_block_duration_seconds",
        Histogram,
        "Wait duration per block."
    ),
    fam!(
        "spacestorage_replication_retries_total",
        Counter,
        "Replication retries."
    ),
    fam!(
        "spacestorage_replication_throughput_bytes_total",
        Counter,
        "Replication throughput bytes."
    ),
    fam!(
        "spacestorage_replication_failures_total",
        Counter,
        "Replication failures."
    ),
];

/// API (FR-010).
pub const API_FAMILIES: &[FamilyMeta] = &[
    fam!(
        "spacestorage_api_connections_total",
        Counter,
        "API connections total."
    ),
    fam!(
        "spacestorage_api_requests_total",
        Counter,
        "API requests total."
    ),
    fam!(
        "spacestorage_api_request_errors_total",
        Counter,
        "API request errors."
    ),
    fam!(
        "spacestorage_api_request_duration_seconds",
        Histogram,
        "API request duration."
    ),
    fam!(
        "spacestorage_api_connections_active",
        Gauge,
        "Active API connections."
    ),
];

/// Background jobs (FR-011/FR-012).
pub const JOB_FAMILIES: &[FamilyMeta] = &[
    fam!(
        "spacestorage_job_duration_seconds",
        Histogram,
        "Background job duration."
    ),
    fam!(
        "spacestorage_job_errors_total",
        Counter,
        "Background job errors."
    ),
    fam!(
        "spacestorage_job_retries_total",
        Counter,
        "Background job retries."
    ),
    fam!(
        "spacestorage_job_queue_size",
        Gauge,
        "Background job queue size."
    ),
    fam!(
        "spacestorage_job_queue_duration_seconds",
        Histogram,
        "Job queue wait duration."
    ),
    fam!(
        "spacestorage_job_queue_errors_total",
        Counter,
        "Job queue errors."
    ),
    fam!(
        "spacestorage_job_queue_retries_total",
        Counter,
        "Job queue retries."
    ),
    fam!(
        "spacestorage_job_queue_completed_total",
        Counter,
        "Job queue completed."
    ),
    fam!(
        "spacestorage_job_queue_failed_total",
        Counter,
        "Job queue failed."
    ),
    fam!(
        "spacestorage_job_queue_starting_total",
        Counter,
        "Job queue starting."
    ),
    fam!(
        "spacestorage_job_queue_running",
        Gauge,
        "Jobs currently running."
    ),
    fam!(
        "spacestorage_job_queue_completed_duration_seconds",
        Histogram,
        "Completed job duration from queue."
    ),
];

/// Datatype LSM/HNSW (FR-013) — frozen names.
pub const DATATYPE_FAMILIES: &[FamilyMeta] = &[
    fam!(
        "db_lsm_memtable_size_bytes",
        Gauge,
        "LSM memtable size."
    ),
    fam!("db_lsm_sstable_count", Gauge, "LSM SSTable count."),
    fam!(
        "db_lsm_compaction_total",
        Counter,
        "LSM compactions."
    ),
    fam!(
        "db_lsm_compaction_duration_seconds",
        Histogram,
        "LSM compaction duration."
    ),
    fam!(
        "db_lsm_compaction_pending",
        Gauge,
        "Pending LSM compactions."
    ),
    fam!("db_lsm_tombstones", Gauge, "LSM tombstones."),
    fam!("db_hnsw_nodes", Gauge, "HNSW node count."),
    fam!("db_hnsw_edges", Gauge, "HNSW edge count."),
    fam!(
        "db_hnsw_search_duration_seconds",
        Histogram,
        "HNSW search duration."
    ),
    fam!("db_hnsw_search_ef", Gauge, "HNSW search ef."),
];

/// Durability (FR-014) — frozen names.
pub const DURABILITY_FAMILIES: &[FamilyMeta] = &[
    fam!("db_wal_bytes_total", Counter, "WAL bytes appended."),
    fam!("db_wal_fsync_total", Counter, "WAL fsyncs."),
    fam!(
        "db_wal_fsync_duration_seconds",
        Histogram,
        "WAL fsync duration."
    ),
    fam!("db_checkpoint_total", Counter, "Checkpoints."),
    fam!(
        "db_checkpoint_duration_seconds",
        Histogram,
        "Checkpoint duration."
    ),
    fam!("db_dirty_blocks", Gauge, "Dirty blocks."),
    fam!("db_dirty_bytes", Gauge, "Dirty bytes."),
    fam!("db_unflushed_bytes", Gauge, "Unflushed bytes."),
    fam!("db_wal_lag_bytes", Gauge, "WAL lag bytes."),
    fam!(
        "db_wal_replay_duration_seconds",
        Histogram,
        "WAL replay duration."
    ),
    fam!(
        "db_wal_recovery_duration_seconds",
        Histogram,
        "WAL recovery duration."
    ),
    fam!("db_recovery_total", Counter, "Recoveries."),
    fam!(
        "db_recovery_duration_seconds",
        Histogram,
        "Recovery duration."
    ),
    fam!(
        "db_recovery_records_total",
        Counter,
        "Recovery records replayed."
    ),
];

/// Billing / quota (`007`, FR-015) — always carry namespace+datatype when known.
pub const BILLING_FAMILIES: &[FamilyMeta] = &[
    fam!(
        "spacestorage_namespace_usage_bytes",
        Gauge,
        "Namespace usage bytes (billing)."
    ),
    fam!(
        "spacestorage_namespace_usage_objects",
        Gauge,
        "Namespace usage objects (billing)."
    ),
    fam!(
        "spacestorage_namespace_connections",
        Gauge,
        "Namespace connections (billing)."
    ),
    fam!(
        "spacestorage_quota_exceeded_total",
        Counter,
        "Quota exceeded events."
    ),
];

/// All catalog family names (for golden tests). HELP/TYPE registered conceptually;
/// series remain absent until recorded.
pub fn all_families() -> Vec<&'static FamilyMeta> {
    SYSTEM_FAMILIES
        .iter()
        .chain(QUERY_FAMILIES.iter())
        .chain(COMM_FAMILIES.iter())
        .chain(REPLICATION_FAMILIES.iter())
        .chain(API_FAMILIES.iter())
        .chain(JOB_FAMILIES.iter())
        .chain(DATATYPE_FAMILIES.iter())
        .chain(DURABILITY_FAMILIES.iter())
        .chain(BILLING_FAMILIES.iter())
        .collect()
}

pub fn family_by_name(name: &str) -> Option<&'static FamilyMeta> {
    all_families().into_iter().find(|f| f.name == name)
}

pub fn help_for(name: &str) -> &'static str {
    family_by_name(name).map(|f| f.help).unwrap_or("")
}
