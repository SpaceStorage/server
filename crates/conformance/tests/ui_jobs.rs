//! UI jobs + monitoring scope (018 T052).

use spacestorage_conformance::{boot_one_node, boot_tenancy_one_node, LAB_ADMIN_TOKEN};
use spacestorage_migrate::{JobKind, JobRecord, JobStatus, Strategy};
use std::time::Duration;
use uuid::Uuid;

async fn http_raw(port: u16, req: &str) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf).into_owned();
    let status = text
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    (status, text)
}

fn body_json(text: &str) -> serde_json::Value {
    serde_json::from_str(text.split("\r\n\r\n").nth(1).unwrap_or("{}")).unwrap_or_default()
}

async fn login_cookie(port: u16) -> (String, String) {
    let host = format!("127.0.0.1:{port}");
    let login = format!(
        "POST /v1/auth/login HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Type: application/json\r\nAuthorization: Bearer {LAB_ADMIN_TOKEN}\r\nContent-Length: 2\r\n\r\n{{}}"
    );
    let (st, body) = http_raw(port, &login).await;
    assert_eq!(st, 200);
    let hdr_end = body.find("\r\n\r\n").unwrap();
    let cookie = body[..hdr_end]
        .lines()
        .find(|h| h.to_ascii_lowercase().starts_with("set-cookie:"))
        .map(|h| h.splitn(2, ':').nth(1).unwrap_or("").trim().to_string())
        .unwrap();
    (host, cookie)
}

async fn http_get(port: u16, host: &str, cookie: &str, path: &str) -> (u16, serde_json::Value) {
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nCookie: {cookie}\r\nConnection: close\r\n\r\n"
    );
    let (st, body) = http_raw(port, &req).await;
    (st, body_json(&body))
}

fn mint_namespace_session(node: &std::sync::Arc<spacestorage_node::Node>, scopes: Vec<String>) -> String {
    let mut store = node.sessions.write().unwrap();
    let (hex, rec) = store.issue(Uuid::new_v4(), "ns-admin", false, 0, Duration::from_secs(3600));
    store.set_namespace_scopes(&rec.token_hash, scopes);
    format!("ss_session={hex}")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ui_jobs_running_progress_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let lab = boot_one_node(tmp.path()).await;
    let mut job = JobRecord::new(
        JobKind::DataMigration,
        Uuid::new_v4(),
        Strategy::Live,
        serde_json::json!({}),
    );
    job.target_internal_name = Some("acme/orders".into());
    job.status = JobStatus::Running;
    job.progress.bytes_copied = 10;
    job.progress.bytes_remaining = Some(90);
    let job_id = job.id;
    lab.node.jobs.store.insert(job);
    let port = lab.ports.admin_http;
    let (host, cookie) = login_cookie(port).await;
    let (st, list) = http_get(port, &host, &cookie, "/v1/ui/jobs").await;
    assert_eq!(st, 200);
    let jobs = list.get("jobs").and_then(|v| v.as_array()).unwrap();
    assert!(!jobs.is_empty());
    let id = job_id.to_string();
    let (st2, det) = http_get(port, &host, &cookie, &format!("/v1/ui/jobs/{id}")).await;
    assert_eq!(st2, 200);
    let progress = det.get("progress").unwrap();
    assert_eq!(progress.get("bytes_copied").and_then(|v| v.as_u64()), Some(10));
    assert_eq!(
        progress.get("bytes_remaining").and_then(|v| v.as_u64()),
        Some(90)
    );
    lab.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn namespace_admin_cluster_monitoring_forbidden() {
    let tmp = tempfile::tempdir().unwrap();
    let lab = boot_tenancy_one_node(tmp.path()).await;
    let port = lab.ports.admin_http;
    let host = format!("127.0.0.1:{port}");
    let cookie = mint_namespace_session(&lab.node, vec!["acme".into()]);
    let (st, _) = http_get(port, &host, &cookie, "/v1/ui/monitoring").await;
    assert_eq!(st, 403);
    let (_, admin_cookie) = login_cookie(port).await;
    let (st3, _) = http_get(port, &host, &admin_cookie, "/v1/ui/monitoring").await;
    assert_eq!(st3, 200);
    lab.shutdown().await;
}