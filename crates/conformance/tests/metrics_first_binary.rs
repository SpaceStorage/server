//! First-binary `/metrics` scrape (008 T015 / SC-004).

use spacestorage_conformance::{boot_one_node, scrape_metrics};
use tempfile::tempdir;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn metrics_first_binary_required_series() {
    let dir = tempdir().unwrap();
    let lab = boot_one_node(dir.path()).await;
    let body = scrape_metrics(lab.ports.admin_http).await;
    assert!(
        body.contains("200") || body.contains("# HELP") || body.contains("# TYPE"),
        "GET /metrics must be 200 Prometheus text, got: {body}"
    );
    for name in [
        "spacestorage_buffer_usage_bytes",
        "spacestorage_buffer_usage_ratio",
        "spacestorage_buffer_limit_hits_total",
        "node_ready",
        "spacestorage_node_ready",
        "node_state",
        "spacestorage_uptime_seconds",
        "spacestorage_worker_threads",
    ] {
        assert!(
            body.contains(name),
            "missing required series {name} in: {body}"
        );
    }
    lab.shutdown().await;
}
