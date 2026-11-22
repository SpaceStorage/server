//! UI monitoring scope (018 T052 subset).

use spacestorage_conformance::{boot_tenancy_one_node, LAB_ADMIN_TOKEN};
use std::time::Duration;

async fn login_cookie(port: u16) -> (String, String) {
    let host = format!("127.0.0.1:{port}");
    let login = format!(
        "POST /v1/auth/login HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Type: application/json\r\nAuthorization: Bearer {LAB_ADMIN_TOKEN}\r\nContent-Length: 2\r\n\r\n{{}}"
    );
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(login.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf).into_owned();
    let hdr_end = text.find("\r\n\r\n").unwrap();
    let cookie = text[..hdr_end]
        .lines()
        .find(|h| h.to_ascii_lowercase().starts_with("set-cookie:"))
        .map(|h| h.splitn(2, ':').nth(1).unwrap_or("").trim().to_string())
        .unwrap();
    (host, cookie)
}

fn mint_namespace_session(node: &std::sync::Arc<spacestorage_node::Node>, scopes: Vec<String>) -> String {
    use uuid::Uuid;
    let mut store = node.sessions.write().unwrap();
    let (hex, rec) = store.issue(Uuid::new_v4(), "ns-admin", false, 0, Duration::from_secs(3600));
    store.set_namespace_scopes(&rec.token_hash, scopes);
    format!("ss_session={hex}")
}

async fn http_get(port: u16, host: &str, cookie: &str, path: &str) -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nCookie: {cookie}\r\nConnection: close\r\n\r\n"
    );
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf).into_owned();
    text.lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .unwrap_or(0)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn namespace_admin_cluster_monitoring_forbidden() {
    let tmp = tempfile::tempdir().unwrap();
    let lab = boot_tenancy_one_node(tmp.path()).await;
    let port = lab.ports.admin_http;
    let host = format!("127.0.0.1:{port}");
    let cookie = mint_namespace_session(&lab.node, vec!["acme".into()]);
    let st = http_get(port, &host, &cookie, "/v1/ui/monitoring").await;
    assert_eq!(st, 403);
    let st2 = http_get(port, &host, &cookie, "/v1/ui/monitoring/namespaces/acme").await;
    assert_eq!(st2, 200);
    let (_, admin_cookie) = login_cookie(port).await;
    let st3 = http_get(port, &host, &admin_cookie, "/v1/ui/monitoring").await;
    assert_eq!(st3, 200);
    lab.shutdown().await;
}