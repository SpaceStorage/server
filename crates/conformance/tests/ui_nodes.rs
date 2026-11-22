//! UI nodes roster (018 T047).

use spacestorage_admin_proto::AdminOp;
use spacestorage_conformance::{admin_tcp_json, boot_one_node, boot_three_node, boot_tenancy_one_node, LAB_ADMIN_TOKEN};
use spacestorage_node::handler::ui_reads::seed_lagging_replica_on;
use spacestorage_types::{L3Model, StorageModeChoice};
use std::time::Duration;

async fn http_get(port: u16, host: &str, cookie: &str, path: &str) -> (u16, serde_json::Value) {
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
    let st = text
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let json: serde_json::Value =
        serde_json::from_str(text.split("\r\n\r\n").nth(1).unwrap_or("{}")).unwrap_or_default();
    (st, json)
}

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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cluster_admin_sees_all_member_nodes() {
    let tmp = tempfile::tempdir().unwrap();
    let (a, b, c) = boot_three_node(tmp.path()).await;
    let port = a.ports.admin_http;
    let (host, cookie) = login_cookie(port).await;
    let (st, json) = http_get(port, &host, &cookie, "/v1/ui/nodes").await;
    assert_eq!(st, 200);
    let nodes = json.get("nodes").and_then(|v| v.as_array()).unwrap();
    assert!(nodes.len() >= 3);
    a.shutdown().await;
    b.shutdown().await;
    c.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn namespace_admin_nodes_subset_of_replica_hosts() {
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
    {
        let mut cat = lab.node.catalog.write().unwrap();
        cat.create(
            "acme",
            "kv",
            L3Model::KvStore,
            false,
            None,
            StorageModeChoice::Memory,
        )
        .unwrap();
    }
    let host_name = lab.node.config.load().node_name.clone();
    seed_lagging_replica_on(&lab.node, "acme", "kv", &host_name, 100).await;
    let port = lab.ports.admin_http;
    let host = format!("127.0.0.1:{port}");
    let (_host2, admin_cookie) = login_cookie(port).await;
    let (st, cluster) = http_get(port, &host, &admin_cookie, "/v1/ui/nodes").await;
    assert_eq!(st, 200);
    let all: Vec<_> = cluster["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|n| n.get("node_name").and_then(|v| v.as_str()))
        .collect();
    let cookie = mint_namespace_session(&lab.node, vec!["acme".into()]);
    let (st2, scoped) = http_get(port, &host, &cookie, "/v1/ui/nodes").await;
    assert_eq!(st2, 200);
    let subset: Vec<_> = scoped["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|n| n.get("node_name").and_then(|v| v.as_str()))
        .collect();
    assert!(!subset.is_empty());
    for n in &subset {
        assert!(all.contains(n), "scoped node {n} must be in cluster roster");
    }
    assert!(subset.contains(&host_name.as_str()));
    assert!(subset.len() <= all.len());
    lab.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn local_node_detail_exposes_threads() {
    let tmp = tempfile::tempdir().unwrap();
    let lab = boot_one_node(tmp.path()).await;
    let port = lab.ports.admin_http;
    let (host, cookie) = login_cookie(port).await;
    let (st, list) = http_get(port, &host, &cookie, "/v1/ui/nodes").await;
    assert_eq!(st, 200);
    let id = list["nodes"][0]["node_id"].as_str().unwrap();
    let (st2, det) = http_get(port, &host, &cookie, &format!("/v1/ui/nodes/{id}")).await;
    assert_eq!(st2, 200);
    assert!(det.get("worker_threads").is_some());
    lab.shutdown().await;
}