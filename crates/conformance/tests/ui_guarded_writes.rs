//! Guarded writes: jobs cancel/resume (018 T058 subset).

use spacestorage_conformance::{boot_one_node, LAB_ADMIN_TOKEN};
use spacestorage_migrate::{JobKind, JobRecord, JobStatus, Strategy};
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

async fn login_session(port: u16) -> (String, String, String) {
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
    let sess = format!(
        "GET /v1/auth/session HTTP/1.1\r\nHost: {host}\r\nCookie: {cookie}\r\nConnection: close\r\n\r\n"
    );
    let (st, body) = http_raw(port, &sess).await;
    assert_eq!(st, 200);
    let json = body_json(&body);
    let csrf = json
        .get("csrf_token")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();
    (host, cookie, csrf)
}

async fn post_empty(
    port: u16,
    host: &str,
    cookie: &str,
    csrf: Option<&str>,
    path: &str,
) -> (u16, serde_json::Value) {
    let req = if let Some(csrf) = csrf {
        format!(
            "POST {path} HTTP/1.1\r\nHost: {host}\r\nCookie: {cookie}\r\nOrigin: http://{host}\r\nX-SpaceStorage-CSRF: {csrf}\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
        )
    } else {
        format!(
            "POST {path} HTTP/1.1\r\nHost: {host}\r\nCookie: {cookie}\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
        )
    };
    let (st, text) = http_raw(port, &req).await;
    (st, body_json(&text))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn job_cancel_cookie_csrf_updates_status() {
    let tmp = tempfile::tempdir().unwrap();
    let lab = boot_one_node(tmp.path()).await;
    let mut job = JobRecord::new(
        JobKind::DataMigration,
        Uuid::new_v4(),
        Strategy::Live,
        serde_json::json!({}),
    );
    job.target_internal_name = Some("acme/x".into());
    job.status = JobStatus::Running;
    let id = job.id;
    lab.node.jobs.store.insert(job);
    let port = lab.ports.admin_http;
    let (host, cookie, csrf) = login_session(port).await;
    let path = format!("/v1/jobs/{id}/cancel");
    let (st, body) = post_empty(port, &host, &cookie, None, &path).await;
    assert_eq!(st, 403);
    assert_eq!(body.get("code").and_then(|v| v.as_str()), Some("csrf_rejected"));
    let (st2, body2) = post_empty(port, &host, &cookie, Some(&csrf), &path).await;
    assert_eq!(st2, 200);
    let status = body2
        .get("job")
        .and_then(|j| j.get("status"))
        .and_then(|v| v.as_str())
        .unwrap();
    assert!(status == "failed" || status == "cancelled");
    lab.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn config_reload_cookie_csrf_accepts() {
    let tmp = tempfile::tempdir().unwrap();
    let lab = boot_one_node(tmp.path()).await;
    let port = lab.ports.admin_http;
    let (host, cookie, csrf) = login_session(port).await;
    let (st, body) = post_empty(port, &host, &cookie, None, "/v1/reload").await;
    assert_eq!(st, 403);
    assert_eq!(body.get("code").and_then(|v| v.as_str()), Some("csrf_rejected"));
    let (st2, body2) = post_empty(port, &host, &cookie, Some(&csrf), "/v1/reload").await;
    assert!(st2 == 200 || st2 == 400, "reload outcome: {st2} {body2:?}");
    lab.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn job_resume_cookie_csrf() {
    let tmp = tempfile::tempdir().unwrap();
    let lab = boot_one_node(tmp.path()).await;
    let mut job = JobRecord::new(
        JobKind::DataMigration,
        Uuid::new_v4(),
        Strategy::Live,
        serde_json::json!({}),
    );
    job.target_internal_name = Some("acme/y".into());
    job.status = JobStatus::Running;
    job.paused = true;
    job.progress.last_applied_source_seq = Some(1);
    let id = job.id;
    lab.node.jobs.store.insert(job);
    let port = lab.ports.admin_http;
    let (host, cookie, csrf) = login_session(port).await;
    let path = format!("/v1/jobs/{id}/resume");
    let (st, body) = post_empty(port, &host, &cookie, None, &path).await;
    assert_eq!(st, 403);
    let (st2, body2) = post_empty(port, &host, &cookie, Some(&csrf), &path).await;
    assert_eq!(st2, 202, "{body2:?}");
    let status = body2
        .get("job")
        .and_then(|j| j.get("status"))
        .and_then(|v| v.as_str());
    assert_eq!(status, Some("running"));
    lab.shutdown().await;
}
