//! Tenant metrics isolation (008 T024) — enabled via Metrics::set_tenant_scrape.

use spacestorage_conformance::boot_one_node;
use spacestorage_observability::{LabelSet, help_for};
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

async fn http_get(port: u16, path: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).await.unwrap();
    String::from_utf8_lossy(&buf).into_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tenant_scrape_isolates_and_404s_when_disabled() {
    let dir = tempdir().unwrap();
    let lab = boot_one_node(dir.path()).await;

    // Disabled → 404 metrics_disabled
    let disabled = http_get(lab.ports.admin_http, "/metrics/namespaces/acme").await;
    assert!(
        disabled.contains("404") && disabled.contains("metrics_disabled"),
        "expected metrics_disabled, got {disabled}"
    );

    // Enable acme scrape and record labeled series
    lab.node.metrics.set_tenant_scrape("acme", true);
    let mut acme = LabelSet::new();
    acme.insert("namespace", "acme").unwrap();
    acme.insert("kind", "get").unwrap();
    lab.node.metrics.registry().inc(
        "spacestorage_query_total",
        acme,
        help_for("spacestorage_query_total"),
        3,
    );
    let mut other = LabelSet::new();
    other.insert("namespace", "other").unwrap();
    other.insert("kind", "get").unwrap();
    lab.node.metrics.registry().inc(
        "spacestorage_query_total",
        other,
        help_for("spacestorage_query_total"),
        9,
    );

    let body = http_get(lab.ports.admin_http, "/metrics/namespaces/acme").await;
    assert!(body.contains("200"), "tenant scrape should be 200: {body}");
    assert!(body.contains("namespace=\"acme\""));
    assert!(!body.contains("namespace=\"other\""));
    // No node-global series (no namespace label) on tenant path
    assert!(!body.contains("spacestorage_uptime_seconds "));

    lab.shutdown().await;
}
