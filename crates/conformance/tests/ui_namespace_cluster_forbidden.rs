//! Namespace-scoped sessions cannot force cluster admin writes (018 T071 / T088).

use spacestorage_admin_proto::AdminOp;
use spacestorage_conformance::{boot_one_node, boot_tenancy_one_node, admin_tcp_json, LAB_ADMIN_TOKEN};
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

fn mint_namespace_session(node: &std::sync::Arc<spacestorage_node::Node>, scopes: Vec<String>) -> String {
    let mut store = node.sessions.write().unwrap();
    let (hex, rec) = store.issue(Uuid::new_v4(), "ns-admin", false, 0, Duration::from_secs(3600));
    store.set_namespace_scopes(&rec.token_hash, scopes);
    format!("ss_session={hex}")
}

async fn session_affordances(port: u16, host: &str, cookie: &str) -> serde_json::Value {
    let req = format!(
        "GET /v1/auth/session HTTP/1.1\r\nHost: {host}\r\nCookie: {cookie}\r\nConnection: close\r\n\r\n"
    );
    let (st, body) = http_raw(port, &req).await;
    assert_eq!(st, 200);
    body_json(&body)
}

async fn admin_http(
    port: u16,
    host: &str,
    cookie: &str,
    csrf: &str,
    op: serde_json::Value,
) -> (u16, serde_json::Value) {
    let body = serde_json::to_string(&op).unwrap();
    let req = format!(
        "POST /v1/admin HTTP/1.1\r\nHost: {host}\r\nCookie: {cookie}\r\nOrigin: http://{host}\r\nX-SpaceStorage-CSRF: {csrf}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let (st, text) = http_raw(port, &req).await;
    (st, body_json(&text))
}

async fn post_empty(port: u16, host: &str, cookie: &str, csrf: &str, path: &str) -> (u16, serde_json::Value) {
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nCookie: {cookie}\r\nOrigin: http://{host}\r\nX-SpaceStorage-CSRF: {csrf}\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
    );
    let (st, text) = http_raw(port, &req).await;
    (st, body_json(&text))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn namespace_session_refuses_cluster_admin_ops() {
    let tmp = tempfile::tempdir().unwrap();
    let lab = boot_one_node(tmp.path()).await;
    let port = lab.ports.admin_http;
    let host = format!("127.0.0.1:{port}");
    let cookie = mint_namespace_session(&lab.node, vec!["acme".into()]);
    let sess = session_affordances(port, &host, &cookie).await;
    let aff = sess.get("affordances").unwrap();
    assert_eq!(aff.get("namespace_write").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(aff.get("quota_replace").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(aff.get("config_reload").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(aff.get("cluster_stop").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(aff.get("node_drain").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(aff.get("job_control").and_then(|v| v.as_bool()), Some(false));

    let csrf = sess
        .get("csrf_token")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();

    for op in [
        serde_json::json!({ "op": "namespace_create", "name": "evil" }),
        serde_json::json!({ "op": "namespace_list" }),
        serde_json::json!({ "op": "quota_replace", "name": "acme", "quotas": [] }),
        serde_json::json!({ "op": "reload" }),
        serde_json::json!({ "op": "membership" }),
    ] {
        let (st, body) = admin_http(port, &host, &cookie, &csrf, op).await;
        assert_eq!(st, 403, "body={body:?}");
        assert_eq!(body.get("code").and_then(|v| v.as_str()), Some("forbidden"));
    }

    let (st, body) = post_empty(port, &host, &cookie, &csrf, "/v1/reload").await;
    assert_eq!(st, 403);
    assert_eq!(body.get("code").and_then(|v| v.as_str()), Some("forbidden"));

    let (st, body) = post_empty(port, &host, &cookie, &csrf, "/v1/stop").await;
    assert_eq!(st, 403);
    assert_eq!(body.get("code").and_then(|v| v.as_str()), Some("forbidden"));

    lab.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn namespace_session_refuses_foreign_describe_and_leases() {
    let tmp = tempfile::tempdir().unwrap();
    let lab = boot_tenancy_one_node(tmp.path()).await;
    admin_tcp_json(
        lab.ports.admin,
        LAB_ADMIN_TOKEN,
        AdminOp::NamespaceCreate {
            name: "otherns".into(),
        },
    )
    .await;
    let port = lab.ports.admin_http;
    let host = format!("127.0.0.1:{port}");
    let cookie = mint_namespace_session(&lab.node, vec!["acme".into()]);
    let sess = session_affordances(port, &host, &cookie).await;
    let csrf = sess
        .get("csrf_token")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();

    let (st, body) = admin_http(
        port,
        &host,
        &cookie,
        &csrf,
        serde_json::json!({ "op": "namespace_describe", "name": "otherns" }),
    )
    .await;
    assert_eq!(st, 403);
    assert_eq!(body.get("code").and_then(|v| v.as_str()), Some("forbidden"));
    assert!(body.get("name").is_none());
    assert!(body.get("containers").is_none());

    let (st, body) = admin_http(
        port,
        &host,
        &cookie,
        &csrf,
        serde_json::json!({ "op": "namespace_describe", "name": "acme" }),
    )
    .await;
    assert_eq!(st, 200);
    assert_eq!(
        body
            .get("namespace")
            .and_then(|n| n.get("name"))
            .and_then(|v| v.as_str()),
        Some("acme")
    );

    let (st, body) = admin_http(
        port,
        &host,
        &cookie,
        &csrf,
        serde_json::json!({ "op": "leases", "namespace": Uuid::new_v4().to_string() }),
    )
    .await;
    assert_eq!(st, 403);
    assert_eq!(body.get("code").and_then(|v| v.as_str()), Some("forbidden"));
    assert!(body.as_array().is_none());

    lab.shutdown().await;
}