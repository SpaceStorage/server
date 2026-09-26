//! Admin TCP / admin-http parity (FR-020) — T078.

use spacestorage_admin_proto::{decode_frame, encode_frame, AdminOp};
use spacestorage_config::model::{
    ClusterDecl, EntrypointDecl, NodeConfig, QueryDefaults, Transport,
};
use spacestorage_node::lifecycle::NodeState;
use spacestorage_node::{runtime, Node};
use std::collections::BTreeMap;
use std::time::Duration;
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

fn lab_config(admin_port: u16, http_port: u16, token_path: &str) -> NodeConfig {
    NodeConfig {
        node_name: "parity-1".into(),
        threads: Some(2),
        drain_timeout: Duration::from_secs(2),
        log_level: "info".into(),
        log_format: "text".into(),
        admin_token_file: Some(token_path.into()),
        disable_admin: false,
        disable_admin_http: false,
        entrypoints: vec![
            EntrypointDecl {
                name: "admin".into(),
                address: "127.0.0.1".into(),
                port: admin_port,
                handler: "admin".into(),
                transport: Transport::Plaintext,
                tls: None,
            },
            EntrypointDecl {
                name: "admin-http".into(),
                address: "127.0.0.1".into(),
                port: http_port,
                handler: "admin-http".into(),
                transport: Transport::Plaintext,
                tls: None,
            },
        ],
        buffers: BTreeMap::new(),
        cluster: ClusterDecl::default(),
        keys: Default::default(),
        query_defaults: QueryDefaults::default(),
        labels: BTreeMap::new(),
        storage_data_dir: None,
        storage: Default::default(),
        limits: spacestorage_config::model::EffectiveLimits::built_in(),
        query: Default::default(),
    }
}

async fn ephemeral_port() -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let p = l.local_addr().unwrap().port();
    drop(l);
    p
}

async fn wait_ready(node: &Node) {
    for _ in 0..100 {
        if node.state.get() == NodeState::Ready {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("not ready");
}

async fn http_json(port: u16, path: &str, token: &str) -> serde_json::Value {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf);
    let body = text.split("\r\n\r\n").nth(1).expect("http body");
    serde_json::from_str(body.trim()).unwrap_or_else(|e| panic!("json parse {e}: {body}"))
}

async fn tcp_op(port: u16, token: &str, op: AdminOp) -> serde_json::Value {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let hello = serde_json::json!({"proto_version": 1, "token": token, "client": "parity-test"});
    let frame = encode_frame(&serde_json::to_vec(&hello).unwrap()).unwrap();
    stream.write_all(&frame).await.unwrap();

    let mut buf = bytes::BytesMut::new();
    // hello ack
    loop {
        let n = stream.read_buf(&mut buf).await.unwrap();
        if n == 0 {
            panic!("eof on hello");
        }
        if let Ok(Some(_)) = decode_frame(&mut buf) {
            break;
        }
    }

    let req = encode_frame(&serde_json::to_vec(&op).unwrap()).unwrap();
    stream.write_all(&req).await.unwrap();
    loop {
        let n = stream.read_buf(&mut buf).await.unwrap();
        if n == 0 {
            panic!("eof on op");
        }
        if let Ok(Some(payload)) = decode_frame(&mut buf) {
            return serde_json::from_slice(&payload).unwrap();
        }
    }
}

fn strip_volatile(mut v: serde_json::Value) -> serde_json::Value {
    if let Some(obj) = v.as_object_mut() {
        obj.remove("uptime_seconds");
        if let Some(threads) = obj.get_mut("threads").and_then(|t| t.as_object_mut()) {
            threads.remove("busy");
        }
        if let Some(eps) = obj.get_mut("entrypoints").and_then(|e| e.as_array_mut()) {
            for ep in eps {
                if let Some(o) = ep.as_object_mut() {
                    o.remove("connections_active");
                }
            }
        }
    }
    v
}

#[tokio::test]
async fn status_config_threads_buffers_match_across_transports() {
    let dir = tempdir().unwrap();
    let token_path = dir.path().join("admin.token");
    std::fs::write(&token_path, "parity-token\n").unwrap();
    let conf = dir.path().join("node.conf");
    std::fs::write(&conf, "#\n").unwrap();

    let admin_port = ephemeral_port().await;
    let http_port = ephemeral_port().await;
    let cfg = lab_config(admin_port, http_port, token_path.to_str().unwrap());
    let (threads, source) = runtime::resolve_worker_threads(cfg.threads);
    let node = Node::boot_first_binary(conf, cfg, threads, source);
    let run = tokio::spawn({
        let n = node.clone();
        async move { n.run().await }
    });
    wait_ready(&node).await;

    let token = "parity-token";

    let http_status = http_json(http_port, "/v1/status", token).await;
    let tcp_status = tcp_op(admin_port, token, AdminOp::Status).await;
    assert_eq!(
        strip_volatile(http_status),
        strip_volatile(tcp_status),
        "status parity"
    );

    let http_config = http_json(http_port, "/v1/config", token).await;
    let tcp_config = tcp_op(admin_port, token, AdminOp::Config).await;
    assert_eq!(http_config, tcp_config, "config parity");
    assert!(
        http_config
            .get("settings")
            .and_then(|s| s.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false),
        "effective config must expose settings with reload_class"
    );

    let http_threads = http_json(http_port, "/v1/threads", token).await;
    let tcp_threads = tcp_op(admin_port, token, AdminOp::Threads).await;
    let mut ht = http_threads.clone();
    let mut tt = tcp_threads.clone();
    ht.as_object_mut().unwrap().remove("busy");
    tt.as_object_mut().unwrap().remove("busy");
    assert_eq!(ht, tt, "threads parity");

    let http_buffers = http_json(http_port, "/v1/buffers", token).await;
    let tcp_buffers = tcp_op(admin_port, token, AdminOp::Buffers).await;
    assert_eq!(http_buffers, tcp_buffers, "buffers parity");

    // /metrics first-binary exposition (016 G1 / T034)
    let mut stream = TcpStream::connect(("127.0.0.1", http_port)).await.unwrap();
    stream
        .write_all(
            b"GET /metrics HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf);
    assert!(text.contains("HTTP/1.1 200"), "metrics must be scrapeable: {text}");
    assert!(
        text.contains("spacestorage_process_uptime_seconds"),
        "metrics body: {text}"
    );

    node.cancel.cancel();
    node.force_cancel.cancel();
    let _ = run.await;
}
