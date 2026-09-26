//! Catalog exercise after owner-like increments (008 T035).

use spacestorage_conformance::{boot_one_node, scrape_metrics};
use spacestorage_observability::{LabelSet, help_for, DURATION_BUCKETS};
use tempfile::tempdir;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn metrics_catalog_after_exercise() {
    let dir = tempdir().unwrap();
    let lab = boot_one_node(dir.path()).await;
    let reg = lab.node.metrics.registry();

    let mut q = LabelSet::new();
    q.insert("kind", "get").unwrap();
    q.insert("namespace", "acme").unwrap();
    q.insert("datatype", "kv").unwrap();
    reg.inc(
        "spacestorage_query_total",
        q.clone(),
        help_for("spacestorage_query_total"),
        1,
    );
    reg.observe(
        "spacestorage_query_duration_seconds",
        q,
        help_for("spacestorage_query_duration_seconds"),
        DURATION_BUCKETS,
        0.01,
    );

    let mut rep = LabelSet::new();
    rep.insert("namespace", "acme").unwrap();
    rep.insert("datatype", "kv").unwrap();
    reg.set_gauge(
        "spacestorage_replication_lag_seconds",
        rep.clone(),
        help_for("spacestorage_replication_lag_seconds"),
        1.5,
    );

    let mut wal = LabelSet::new();
    wal.insert("drive", "d0").unwrap();
    reg.inc(
        "db_wal_bytes_total",
        wal,
        help_for("db_wal_bytes_total"),
        64,
    );

    let mut lsm = LabelSet::new();
    lsm.insert("namespace", "acme").unwrap();
    lsm.insert("datatype", "kv").unwrap();
    reg.set_gauge(
        "db_lsm_sstable_count",
        lsm,
        help_for("db_lsm_sstable_count"),
        2.0,
    );

    let mut job = LabelSet::new();
    job.insert("job", "compaction").unwrap();
    job.insert("status", "completed").unwrap();
    reg.inc(
        "spacestorage_job_queue_completed_total",
        job,
        help_for("spacestorage_job_queue_completed_total"),
        1,
    );

    let mut bill = LabelSet::new();
    bill.insert("namespace", "acme").unwrap();
    bill.insert("datatype", "kv").unwrap();
    reg.set_gauge(
        "spacestorage_namespace_usage_bytes",
        bill,
        help_for("spacestorage_namespace_usage_bytes"),
        1024.0,
    );

    let body = scrape_metrics(lab.ports.admin_http).await;
    for name in [
        "spacestorage_query_total",
        "spacestorage_query_duration_seconds",
        "spacestorage_replication_lag_seconds",
        "db_wal_bytes_total",
        "db_lsm_sstable_count",
        "spacestorage_job_queue_completed_total",
        "spacestorage_namespace_usage_bytes",
        "spacestorage_uptime_seconds",
        "node_ready",
    ] {
        assert!(body.contains(name), "missing {name} in {body}");
    }
    assert!(!body.contains("user=\"\""));
    assert!(!body.contains("user=\"unknown\""));

    lab.shutdown().await;
}
