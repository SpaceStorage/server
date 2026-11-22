//! UI overview JSON (018 T033).

use spacestorage_admin_proto::AdminOp;
use spacestorage_conformance::{
    boot_one_node, boot_tenancy_one_node, boot_three_node, admin_tcp_json, LAB_ADMIN_TOKEN,
};
use spacestorage_node::handler::ui_reads::seed_lagging_replica;
use spacestorage_types::{ContainerDefinition, StorageModeChoice};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

async fn http_raw(port: u16, req: &str) -> (u16, String, Vec<String>) {
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
    let headers: Vec<String> = text
        .lines()
        .skip(1)
        .take_while(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect();
    (status, text, headers)
}

fn parse_set_cookie(headers: &str) -> Option<String> {
    headers
        .lines()
        .find(|h| h.to_ascii_lowercase().starts_with("set-cookie:"))
        .map(|h| h.splitn(2, ':').nth(1).unwrap_or("").trim().to_string())
}

async fn login_cluster_admin(port: u16) -> (String, String) {
    let host = format!("127.0.0.1:{port}");
    let login = format!(
        "POST /v1/auth/login HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Type: application/json\r\nAuthorization: Bearer {LAB_ADMIN_TOKEN}\r\nContent-Length: 2\r\n\r\n{{}}"
    );
    let (st, body, _) = http_raw(port, &login).await;
    assert_eq!(st, 200);
    let hdr_end = body.find("\r\n\r\n").unwrap();
    let cookie = parse_set_cookie(&body[..hdr_end]).unwrap();
    (host, cookie)
}

fn mint_namespace_session(node: &Arc<spacestorage_node::Node>, scopes: Vec<String>) -> String {
    let mut store = node.sessions.write().unwrap();
    let (hex, rec) = store.issue(
        Uuid::new_v4(),
        "ns-admin",
        false,
        0,
        Duration::from_secs(3600),
    );
    store.set_namespace_scopes(&rec.token_hash, scopes);
    format!("ss_session={hex}")
}

async fn get_overview(port: u16, host: &str, cookie: &str) -> (u16, serde_json::Value) {
    let ov = format!(
        "GET /v1/ui/overview HTTP/1.1\r\nHost: {host}\r\nCookie: {cookie}\r\nConnection: close\r\n\r\n"
    );
    let (st, body, _) = http_raw(port, &ov).await;
    let json: serde_json::Value =
        serde_json::from_str(body.split("\r\n\r\n").nth(1).unwrap()).unwrap_or(serde_json::json!({}));
    (st, json)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn overview_json_single_node() {
    let tmp = tempfile::tempdir().unwrap();
    let lab = boot_one_node(tmp.path()).await;
    let port = lab.ports.admin_http;
    let (host, cookie) = login_cluster_admin(port).await;
    let (st, json) = get_overview(port, &host, &cookie).await;
    assert_eq!(st, 200);
    assert!(json.get("generated_at").is_some());
    assert!(json.get("members").is_some());
    assert!(json.get("source").is_some());
    let members = json.get("members").unwrap();
    let ready = members.get("ready").and_then(|v| v.as_u64()).unwrap_or(0);
    let not_ready = members.get("not_ready").and_then(|v| v.as_u64()).unwrap_or(0);
    assert!(ready + not_ready >= 1);
    assert!(members.get("degraded").and_then(|v| v.as_array()).is_some());
    lab.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn overview_three_node_degraded_and_lagging_replica() {
    let tmp = tempfile::tempdir().unwrap();
    let (a, b, c) = boot_three_node(tmp.path()).await;
    let port = a.ports.admin_http;
    {
        let view = a
            .node
            .membership
            .read()
            .await
            .as_ref()
            .and_then(|m| m.view_snapshot())
            .expect("roster");
        let c_id = view
            .members
            .iter()
            .find(|(_, rec)| rec.node_name == "db-c")
            .map(|(id, _)| *id)
            .expect("db-c");
        admin_tcp_json(a.ports.admin, LAB_ADMIN_TOKEN, AdminOp::Drain { node_id: c_id }).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    admin_tcp_json(
        a.ports.admin,
        LAB_ADMIN_TOKEN,
        AdminOp::NamespaceCreate {
            name: "demo".into(),
        },
    )
    .await;
    {
        let mut cat = a.node.catalog.write().unwrap();
        cat.create_from_definition(
            "demo",
            "lagbox",
            &ContainerDefinition {
                type_name: "kv_store".into(),
                mode: StorageModeChoice::Persistent,
                multi_active: false,
                schema: None,
                encryption: None,
            },
        )
        .unwrap();
    }
    seed_lagging_replica(&a.node, "demo", "lagbox", 4200).await;
    let (host, cookie) = login_cluster_admin(port).await;
    let (st, json) = get_overview(port, &host, &cookie).await;
    assert_eq!(st, 200);
    let degraded = json["members"]["degraded"].as_array().unwrap();
    assert!(
        degraded.iter().any(|m| {
            m.get("readiness").and_then(|v| v.as_str()) == Some("not_ready")
        }),
        "expected drained member in degraded"
    );
    let named = json["replicas"]["named"].as_array().unwrap();
    assert!(
        named.iter().any(|r| {
            r.get("lag_ms").and_then(|v| v.as_u64()) == Some(4200)
        }),
        "expected lagging replica"
    );
    a.shutdown().await;
    b.shutdown().await;
    c.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn overview_namespace_admin_scoped_counts() {
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
        cat.create_from_definition(
            "acme",
            "mine",
            &ContainerDefinition {
                type_name: "kv_store".into(),
                mode: StorageModeChoice::Persistent,
                multi_active: false,
                schema: None,
                encryption: None,
            },
        )
        .unwrap();
        cat.create_from_definition(
            "otherns",
            "foreign",
            &ContainerDefinition {
                type_name: "kv_store".into(),
                mode: StorageModeChoice::Persistent,
                multi_active: false,
                schema: None,
                encryption: None,
            },
        )
        .unwrap();
    }
    seed_lagging_replica(&lab.node, "otherns", "foreign", 900).await;
    let port = lab.ports.admin_http;
    let host = format!("127.0.0.1:{port}");
    let cookie = mint_namespace_session(&lab.node, vec!["acme".into()]);
    let (st, json) = get_overview(port, &host, &cookie).await;
    assert_eq!(st, 200);
    assert_eq!(json["counts"]["namespaces"], 1);
    assert_eq!(json["counts"]["containers"], 1);
    let named = json["replicas"]["named"].as_array().unwrap();
    assert!(
        !named.iter().any(|r| r.get("namespace").and_then(|v| v.as_str()) == Some("otherns"))
    );
    lab.shutdown().await;
}