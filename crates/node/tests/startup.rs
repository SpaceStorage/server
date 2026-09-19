use spacestorage_config::model::{
    ClusterDecl, EntrypointDecl, NodeConfig, QueryDefaults, Transport,
};
use spacestorage_node::{runtime, Node};
use std::collections::BTreeMap;
use std::time::Duration;
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

fn lab_config(admin_port: u16, http_port: u16, token_path: &str) -> NodeConfig {
    NodeConfig {
        node_name: "test-1".into(),
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
        query_defaults: QueryDefaults::default(),
        labels: BTreeMap::new(),
        storage_data_dir: None,
    }
}

#[tokio::test]
async fn startup_reaches_ready_and_http_status() {
    let dir = tempdir().unwrap();
    let token = dir.path().join("admin.token");
    std::fs::write(&token, "secret-token\n").unwrap();
    let conf = dir.path().join("node.conf");
    std::fs::write(&conf, "# placeholder\n").unwrap();

    let l1 = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let admin_port = l1.local_addr().unwrap().port();
    drop(l1);
    let l2 = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let http_port = l2.local_addr().unwrap().port();
    drop(l2);

    let cfg = lab_config(admin_port, http_port, token.to_str().unwrap());
    let (threads, source) = runtime::resolve_worker_threads(cfg.threads);
    let node = Node::boot_first_binary(conf, cfg, threads, source);
    let node2 = node.clone();
    let run = tokio::spawn(async move { node2.run().await });

    for _ in 0..50 {
        if node.state.get() == spacestorage_node::lifecycle::NodeState::Ready {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        node.state.get(),
        spacestorage_node::lifecycle::NodeState::Ready
    );

    let mut stream = TcpStream::connect(("127.0.0.1", http_port))
        .await
        .expect("connect admin-http");
    let req = format!(
        "GET /v1/status HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer secret-token\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf);
    assert!(text.contains("HTTP/1.1 200"), "response: {text}");
    assert!(text.contains("\"state\":\"ready\"") || text.contains("\"state\": \"ready\""));
    assert!(text.contains("first-binary"));

    node.cancel.cancel();
    let _ = run.await;
}
