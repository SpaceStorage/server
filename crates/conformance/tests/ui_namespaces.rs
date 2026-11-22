//! UI namespaces lifecycle (018 T041 / quickstart §6).

use spacestorage_admin_proto::AdminOp;
use spacestorage_conformance::{admin_tcp_json, boot_one_node, boot_tenancy_one_node, LAB_ADMIN_TOKEN};
use spacestorage_types::{L3Model, StorageModeChoice};
use std::time::Duration;

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

fn parse_set_cookie(headers: &str) -> Option<String> {
    headers
        .lines()
        .find(|h| h.to_ascii_lowercase().starts_with("set-cookie:"))
        .map(|h| h.splitn(2, ':').nth(1).unwrap_or("").trim().to_string())
}

async fn login_session(port: u16) -> (String, String, String) {
    let host = format!("127.0.0.1:{port}");
    let login = format!(
        "POST /v1/auth/login HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Type: application/json\r\nAuthorization: Bearer {LAB_ADMIN_TOKEN}\r\nContent-Length: 2\r\n\r\n{{}}"
    );
    let (st, body) = http_raw(port, &login).await;
    assert_eq!(st, 200);
    let hdr_end = body.find("\r\n\r\n").unwrap();
    let cookie = parse_set_cookie(&body[..hdr_end]).unwrap();
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

async fn http_get(port: u16, host: &str, cookie: &str, path: &str) -> (u16, serde_json::Value) {
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nCookie: {cookie}\r\nConnection: close\r\n\r\n"
    );
    let (st, body) = http_raw(port, &req).await;
    (st, body_json(&body))
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

fn ns_names(list: &serde_json::Value) -> Vec<String> {
    list.get("namespaces")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|n| n.get("name").and_then(|v| v.as_str()).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn mint_namespace_session(node: &std::sync::Arc<spacestorage_node::Node>, scopes: Vec<String>) -> String {
    use uuid::Uuid;
    let mut store = node.sessions.write().unwrap();
    let (hex, rec) = store.issue(Uuid::new_v4(), "ns-admin", false, 0, Duration::from_secs(3600));
    store.set_namespace_scopes(&rec.token_hash, scopes);
    format!("ss_session={hex}")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn namespace_lifecycle_quickstart_six() {
    let tmp = tempfile::tempdir().unwrap();
    let lab = boot_one_node(tmp.path()).await;
    let port = lab.ports.admin_http;
    let (host, cookie, csrf) = login_session(port).await;

    let (st, list) = http_get(port, &host, &cookie, "/v1/ui/namespaces").await;
    assert_eq!(st, 200);
    assert!(ns_names(&list).is_empty());

    let (st, _) = admin_http(
        port,
        &host,
        &cookie,
        &csrf,
        serde_json::json!({ "op": "namespace_create", "name": "acme" }),
    )
    .await;
    assert_eq!(st, 200);
    tokio::time::sleep(Duration::from_millis(50)).await;

    let (st, list) = http_get(port, &host, &cookie, "/v1/ui/namespaces").await;
    assert_eq!(st, 200);
    assert!(ns_names(&list).contains(&"acme".to_string()));

    let (st, err) = admin_http(
        port,
        &host,
        &cookie,
        &csrf,
        serde_json::json!({ "op": "namespace_create", "name": "1bad" }),
    )
    .await;
    assert_eq!(st, 400);
    assert!(err.get("message").is_some());
    let (st, list2) = http_get(port, &host, &cookie, "/v1/ui/namespaces").await;
    assert_eq!(st, 200);
    assert_eq!(ns_names(&list2).len(), 1);

    {
        let mut cat = lab.node.catalog.write().unwrap();
        cat.create(
            "acme",
            "items",
            L3Model::KvStore,
            false,
            None,
            StorageModeChoice::Memory,
        )
        .unwrap();
    }

    let (st, err) = admin_http(
        port,
        &host,
        &cookie,
        &csrf,
        serde_json::json!({ "op": "namespace_delete", "name": "acme", "cascade": false }),
    )
    .await;
    assert_eq!(st, 400);
    assert_eq!(err.get("code").and_then(|v| v.as_str()), Some("CascadeRequired"));

    let (st, _) = admin_http(
        port,
        &host,
        &cookie,
        &csrf,
        serde_json::json!({ "op": "namespace_delete", "name": "acme", "cascade": true }),
    )
    .await;
    assert_eq!(st, 200);
    let (st, list3) = http_get(port, &host, &cookie, "/v1/ui/namespaces").await;
    assert_eq!(st, 200);
    assert!(!ns_names(&list3).contains(&"acme".to_string()));

    let (st, _) = admin_http(
        port,
        &host,
        &cookie,
        &csrf,
        serde_json::json!({ "op": "namespace_create", "name": "acme" }),
    )
    .await;
    assert_eq!(st, 200);
    let (st, _) = admin_http(
        port,
        &host,
        &cookie,
        &csrf,
        serde_json::json!({ "op": "namespace_rename", "name": "acme", "new_name": "acme2" }),
    )
    .await;
    assert_eq!(st, 200);
    let (st, _) = http_get(port, &host, &cookie, "/v1/ui/namespaces/acme").await;
    assert_eq!(st, 403);
    let (st, det) = http_get(port, &host, &cookie, "/v1/ui/namespaces/acme2").await;
    assert_eq!(st, 200);
    assert_eq!(det.get("name").and_then(|v| v.as_str()), Some("acme2"));

    let (st, _) = http_get(
        port,
        &host,
        &cookie,
        "/v1/ui/namespaces?limit=101",
    )
    .await;
    assert_eq!(st, 400);

    lab.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn namespace_admin_foreign_detail_forbidden() {
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
    let (st, _) = http_get(port, &host, &cookie, "/v1/ui/namespaces/otherns").await;
    assert_eq!(st, 403);
    let (st2, list) = http_get(port, &host, &cookie, "/v1/ui/namespaces").await;
    assert_eq!(st2, 200);
    let names = ns_names(&list);
    assert!(names.contains(&"acme".to_string()));
    assert!(!names.contains(&"otherns".to_string()));
    lab.shutdown().await;
}

#[cfg(feature = "tenancy-quotas")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn quota_replace_updates_summary_on_200() {
    let tmp = tempfile::tempdir().unwrap();
    let lab = boot_tenancy_one_node(tmp.path()).await;
    let port = lab.ports.admin_http;
    let (host, cookie, csrf) = login_session(port).await;
    let (st, before) = http_get(port, &host, &cookie, "/v1/ui/namespaces/acme").await;
    assert_eq!(st, 200);
    assert!(before.get("quota").unwrap().is_null());

    let (st, err) = admin_http(
        port,
        &host,
        &cookie,
        &csrf,
        serde_json::json!({
            "op": "quota_replace",
            "name": "acme",
            "quotas": [{ "unit": "objects", "limit": 0 }]
        }),
    )
    .await;
    assert!(st == 400 || st == 200);
    if st == 400 {
        assert!(err.get("message").is_some());
        let (st2, still) = http_get(port, &host, &cookie, "/v1/ui/namespaces/acme").await;
        assert_eq!(st2, 200);
        assert!(still.get("quota").unwrap().is_null());
    }

    let (st, _) = admin_http(
        port,
        &host,
        &cookie,
        &csrf,
        serde_json::json!({
            "op": "quota_replace",
            "name": "acme",
            "quotas": [{ "unit": "bytes", "limit": 1073741824u64 }]
        }),
    )
    .await;
    assert_eq!(st, 200);
    let (st, after) = http_get(port, &host, &cookie, "/v1/ui/namespaces/acme").await;
    assert_eq!(st, 200);
    assert_eq!(
        after.get("quota")
            .and_then(|q| q.get("bytes"))
            .and_then(|v| v.as_u64()),
        Some(1073741824)
    );
    lab.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn quota_replace_refused_without_feature_leaves_summary() {
    let tmp = tempfile::tempdir().unwrap();
    let lab = boot_tenancy_one_node(tmp.path()).await;
    let port = lab.ports.admin_http;
    let (host, cookie, csrf) = login_session(port).await;
    let (st, before) = http_get(port, &host, &cookie, "/v1/ui/namespaces/acme").await;
    assert_eq!(st, 200);
    let (st, err) = admin_http(
        port,
        &host,
        &cookie,
        &csrf,
        serde_json::json!({
            "op": "quota_replace",
            "name": "acme",
            "quotas": [{ "unit": "bytes", "limit": 1 }]
        }),
    )
    .await;
    if !cfg!(feature = "tenancy-quotas") {
        assert_eq!(st, 400);
        assert!(err.get("message").is_some());
        let (st2, after) = http_get(port, &host, &cookie, "/v1/ui/namespaces/acme").await;
        assert_eq!(st2, 200);
        assert_eq!(before.get("quota"), after.get("quota"));
    }
    lab.shutdown().await;
}