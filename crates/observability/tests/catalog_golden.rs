//! Catalog golden: required HELP/TYPE names from contracts/catalog.md.

use spacestorage_observability::{
    all_families, SampleKind, DATATYPE_FAMILIES, DURABILITY_FAMILIES, JOB_FAMILIES, JOB_NAMES,
    QUERY_FAMILIES, REPLICATION_FAMILIES, SYSTEM_FAMILIES, API_FAMILIES, BILLING_FAMILIES,
    COMM_FAMILIES,
};

#[test]
fn catalog_has_four_golden_signal_families() {
    // Latency / traffic / errors / saturation covered by query+api+comm+system.
    assert!(QUERY_FAMILIES.iter().any(|f| f.name == "spacestorage_query_duration_seconds"));
    assert!(QUERY_FAMILIES.iter().any(|f| f.name == "spacestorage_query_total"));
    assert!(QUERY_FAMILIES.iter().any(|f| f.name == "spacestorage_query_errors_total"));
    assert!(SYSTEM_FAMILIES.iter().any(|f| f.name == "spacestorage_buffer_usage_ratio"));
    assert!(API_FAMILIES.iter().any(|f| f.name == "spacestorage_api_request_duration_seconds"));
    assert!(COMM_FAMILIES.iter().any(|f| f.name == "spacestorage_comm_query_total"));
}

#[test]
fn durability_and_datatype_names_frozen() {
    let dur: Vec<_> = DURABILITY_FAMILIES.iter().map(|f| f.name).collect();
    for n in [
        "db_wal_bytes_total",
        "db_wal_fsync_total",
        "db_wal_fsync_duration_seconds",
        "db_checkpoint_total",
        "db_checkpoint_duration_seconds",
        "db_dirty_blocks",
        "db_dirty_bytes",
        "db_unflushed_bytes",
        "db_wal_lag_bytes",
        "db_wal_replay_duration_seconds",
        "db_wal_recovery_duration_seconds",
        "db_recovery_total",
        "db_recovery_duration_seconds",
        "db_recovery_records_total",
    ] {
        assert!(dur.contains(&n), "missing durability {n}");
    }
    let dt: Vec<_> = DATATYPE_FAMILIES.iter().map(|f| f.name).collect();
    for n in [
        "db_lsm_memtable_size_bytes",
        "db_lsm_sstable_count",
        "db_lsm_compaction_total",
        "db_lsm_compaction_duration_seconds",
        "db_lsm_compaction_pending",
        "db_lsm_tombstones",
        "db_hnsw_nodes",
        "db_hnsw_edges",
        "db_hnsw_search_duration_seconds",
        "db_hnsw_search_ef",
    ] {
        assert!(dt.contains(&n), "missing datatype {n}");
    }
}

#[test]
fn billing_series_present() {
    let names: Vec<_> = BILLING_FAMILIES.iter().map(|f| f.name).collect();
    for n in [
        "spacestorage_namespace_usage_bytes",
        "spacestorage_namespace_usage_objects",
        "spacestorage_namespace_connections",
        "spacestorage_quota_exceeded_total",
    ] {
        assert!(names.contains(&n), "missing billing {n}");
    }
}

#[test]
fn job_names_cover_fr012() {
    assert_eq!(JOB_NAMES.len(), 15);
    assert!(JOB_FAMILIES.iter().any(|f| f.name == "spacestorage_job_duration_seconds"));
    assert!(REPLICATION_FAMILIES
        .iter()
        .any(|f| f.name == "spacestorage_replication_lag_seconds"));
}

#[test]
fn all_families_have_help_and_type() {
    for f in all_families() {
        assert!(!f.help.is_empty(), "{} missing HELP", f.name);
        let _ = matches!(
            f.kind,
            SampleKind::Counter | SampleKind::Gauge | SampleKind::Histogram
        );
    }
}

#[test]
fn first_binary_system_aliases() {
    let names: Vec<_> = SYSTEM_FAMILIES.iter().map(|f| f.name).collect();
    assert!(names.contains(&"node_ready"));
    assert!(names.contains(&"spacestorage_node_ready"));
    assert!(names.contains(&"node_state"));
    assert!(names.contains(&"spacestorage_node_state"));
    assert!(names.contains(&"spacestorage_uptime_seconds"));
    assert!(names.contains(&"spacestorage_worker_threads"));
    assert!(names.contains(&"spacestorage_buffer_usage_bytes"));
    assert!(names.contains(&"spacestorage_buffer_limit_hits_total"));
}
