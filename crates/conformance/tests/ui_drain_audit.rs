//! Drain/undrain + audit page (018 T056).

use spacestorage_conformance::{boot_one_node, LAB_ADMIN_TOKEN};
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

async fn admin_http(
    port: u16,
    host: &str,
    cookie: &str,
    csrf: Option<&str>,
    op: serde_json::Value,
) -> (u16, serde_json::Value) {
    let body = serde_json::to_string(&op).unwrap();
    let req = if let Some(csrf) = csrf {
        format!(
            "POST /v1/admin HTTP/1.1\r\nHost: {host}\r\nCookie: {cookie}\r\nOrigin: http://{host}\r\nX-SpaceStorage-CSRF: {csrf}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
    } else {
        format!(
            "POST /v1/admin HTTP/1.1\r\nHost: {host}\r\nCookie: {cookie}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
    };
    let (st, text) = http_raw(port, &req).await;
    (st, body_json(&text))
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
async fn drain_csrf_refusal_leaves_membership_unchanged() {
    let tmp = tempfile::tempdir().unwrap();
    let lab = boot_one_node(tmp.path()).await;
    let port = lab.ports.admin_http;
    let (host, cookie, csrf) = login_session(port).await;
    let (st, nodes) = http_get(port, &host, &cookie, "/v1/ui/nodes").await;
    assert_eq!(st, 200);
    let node_id = nodes["nodes"][0]["node_id"].as_str().unwrap();
    let before = nodes["nodes"][0]["readiness"].as_str().unwrap().to_string();
    let op = serde_json::json!({ "op": "drain", "node_id": node_id });
    let (st2, body) = admin_http(port, &host, &cookie, None, op.clone()).await;
    assert_eq!(st2, 403);
    assert_eq!(body.get("code").and_then(|v| v.as_str()), Some("csrf_rejected"));
    let (st3, nodes2) = http_get(port, &host, &cookie, "/v1/ui/nodes").await;
    assert_eq!(st3, 200);
    assert_eq!(
        nodes2["nodes"][0]["readiness"].as_str().unwrap(),
        before
    );
    let (st4, body2) = admin_http(port, &host, &cookie, Some(&csrf), op.clone()).await;
    assert_eq!(st4, 200);
    assert_eq!(body2.get("status").and_then(|v| v.as_str()), Some("draining"));
    let (st5, nodes3) = http_get(port, &host, &cookie, "/v1/ui/nodes").await;
    assert_eq!(st5, 200);
    assert_ne!(nodes3["nodes"][0]["readiness"].as_str().unwrap(), "ready");
    let undrain = serde_json::json!({ "op": "undrain", "node_id": node_id });
    let (st6, body3) = admin_http(port, &host, &cookie, Some(&csrf), undrain).await;
    assert_eq!(st6, 200);
    assert_eq!(body3.get("status").and_then(|v| v.as_str()), Some("ready"));
    lab.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn namespace_admin_drain_forbidden() {
    let tmp = tempfile::tempdir().unwrap();
    let lab = boot_one_node(tmp.path()).await;
    let port = lab.ports.admin_http;
    let (host, admin_cookie, _) = login_session(port).await;
    let (_, nodes) = http_get(port, &host, &admin_cookie, "/v1/ui/nodes").await;
    let node_id = nodes["nodes"][0]["node_id"].as_str().unwrap();
    let ns_cookie = mint_namespace_session(&lab.node, vec!["acme".into()]);
    let sess = format!(
        "GET /v1/auth/session HTTP/1.1\r\nHost: {host}\r\nCookie: {ns_cookie}\r\nConnection: close\r\n\r\n"
    );
    let (st, body) = http_raw(port, &sess).await;
    assert_eq!(st, 200);
    let csrf = body_json(&body)
        .get("csrf_token")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();
    let op = serde_json::json!({ "op": "drain", "node_id": node_id });
    let (st, body) = admin_http(port, &host, &ns_cookie, Some(&csrf), op).await;
    assert_eq!(st, 403);
    assert_ne!(body.get("code").and_then(|v| v.as_str()), Some("csrf_rejected"));
    lab.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn drain_confirmed_process_stays_up() {
    let tmp = tempfile::tempdir().unwrap();
    let lab = boot_one_node(tmp.path()).await;
    let port = lab.ports.admin_http;
    let (host, cookie, csrf) = login_session(port).await;
    let (_, nodes) = http_get(port, &host, &cookie, "/v1/ui/nodes").await;
    let node_id = nodes["nodes"][0]["node_id"].as_str().unwrap();
    let op = serde_json::json!({ "op": "drain", "node_id": node_id });
    let (st, _) = admin_http(port, &host, &cookie, Some(&csrf), op).await;
    assert_eq!(st, 200);
    let live = format!(
        "GET /v1/health/live HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    );
    let (st_live, _) = http_raw(port, &live).await;
    assert_eq!(st_live, 200);
    lab.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ui_audit_page_matches_fixture_shape() {
    let tmp = tempfile::tempdir().unwrap();
    let lab = boot_one_node(tmp.path()).await;
    let port = lab.ports.admin_http;
    let (host, cookie, _) = login_session(port).await;
    let (st, page) = http_get(port, &host, &cookie, "/v1/ui/audit").await;
    assert_eq!(st, 200);
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../specs/018-admin-ui-modernization/contracts/fixtures/audit-page.json"
    ))
    .unwrap();
    for key in ["generated_at", "limit", "next_cursor", "entries"] {
        assert!(page.get(key).is_some());
        assert_eq!(page.get(key).map(|v| v.is_null()), fixture.get(key).map(|v| v.is_null()));
    }
    lab.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ui_audit_page_has_no_export_keys() {
    let tmp = tempfile::tempdir().unwrap();
    let lab = boot_one_node(tmp.path()).await;
    let port = lab.ports.admin_http;
    let (host, cookie, _) = login_session(port).await;
    let (st, page) = http_get(port, &host, &cookie, "/v1/ui/audit").await;
    assert_eq!(st, 200);
    assert!(page.get("export").is_none());
    assert!(page.get("retention").is_none());
    assert!(page.get("entries").is_some());
    let s = serde_json::to_string(&page).unwrap();
    assert!(!s.contains("export"));
    assert!(!s.contains("retention"));
    lab.shutdown().await;
}