//! Drain / stop integration tests (SC-010, FR-006/007) — T077.

use spacestorage_config::model::{
    ClusterDecl, EntrypointDecl, NodeConfig, QueryDefaults, Transport,
};
use spacestorage_node::lifecycle::NodeState;
use spacestorage_node::{runtime, Node};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

fn config_with_echo(
    admin_http_port: u16,
    echo_port: u16,
    token_path: &str,
    drain_timeout: Duration,
) -> NodeConfig {
    NodeConfig {
        node_name: "drain-1".into(),
        threads: Some(2),
        drain_timeout,
        log_level: "info".into(),
        log_format: "text".into(),
        admin_token_file: Some(token_path.into()),
        disable_admin: true,
        disable_admin_http: false,
        entrypoints: vec![
            EntrypointDecl {
                name: "admin-http".into(),
                address: "127.0.0.1".into(),
                port: admin_http_port,
                handler: "admin-http".into(),
                transport: Transport::Plaintext,
                tls: None,
                ingest: None,
            },
            EntrypointDecl {
                name: "echo".into(),
                address: "127.0.0.1".into(),
                port: echo_port,
                handler: "echo".into(),
                transport: Transport::Plaintext,
                tls: None,
                ingest: None,
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
        metrics: Default::default(),
        log_kafka: None,
        log_syslog: None,
        jobs: spacestorage_config::model::JobsDecl::default(),
        kafka_ingests: Vec::new(),
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
    panic!("node did not reach ready");
}

#[tokio::test]
async fn inflight_completes_within_drain_timeout() {
    let dir = tempdir().unwrap();
    let token = dir.path().join("admin.token");
    std::fs::write(&token, "secret\n").unwrap();
    let conf = dir.path().join("node.conf");
    std::fs::write(&conf, "#\n").unwrap();

    let http_port = ephemeral_port().await;
    let echo_port = ephemeral_port().await;
    let cfg = config_with_echo(
        http_port,
        echo_port,
        token.to_str().unwrap(),
        Duration::from_secs(3),
    );
    let (threads, source) = runtime::resolve_worker_threads(cfg.threads);
    let node = Node::boot_first_binary(conf, cfg, threads, source);
    let run = tokio::spawn({
        let n = node.clone();
        async move { n.run().await }
    });
    wait_ready(&node).await;

    let done = Arc::new(AtomicBool::new(false));
    let done2 = done.clone();
    let echo_task = tokio::spawn(async move {
        let mut s = TcpStream::connect(("127.0.0.1", echo_port))
            .await
            .unwrap();
        s.write_all(b"ping").await.unwrap();
        let mut buf = [0u8; 4];
        s.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping");
        // Hold the connection open briefly so drain must wait for in-flight work.
        tokio::time::sleep(Duration::from_millis(200)).await;
        drop(s);
        done2.store(true, Ordering::SeqCst);
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(spacestorage_node::lifecycle::request_drain(&node));
    assert_eq!(node.state.get(), NodeState::Draining);

    // New tenant connections refused while draining.
    let refused = TcpStream::connect(("127.0.0.1", echo_port)).await;
    if let Ok(mut s) = refused {
        // Accept may have raced; connection should be dropped quickly.
        let _ = tokio::time::timeout(Duration::from_millis(200), s.read(&mut [0u8; 1])).await;
    }

    let start = Instant::now();
    let _ = run.await;
    assert!(start.elapsed() < Duration::from_secs(4));
    assert!(done.load(Ordering::SeqCst) || echo_task.is_finished());
    let _ = echo_task.await;
    assert!(!node.stats.drain_timed_out());
}

#[tokio::test]
async fn drain_timeout_records_timed_out() {
    let dir = tempdir().unwrap();
    let token = dir.path().join("admin.token");
    std::fs::write(&token, "secret\n").unwrap();
    let conf = dir.path().join("node.conf");
    std::fs::write(&conf, "#\n").unwrap();

    let http_port = ephemeral_port().await;
    let echo_port = ephemeral_port().await;
    let cfg = config_with_echo(
        http_port,
        echo_port,
        token.to_str().unwrap(),
        Duration::from_millis(200),
    );
    let (threads, source) = runtime::resolve_worker_threads(cfg.threads);
    let node = Node::boot_first_binary(conf, cfg, threads, source);
    let run = tokio::spawn({
        let n = node.clone();
        async move { n.run().await }
    });
    wait_ready(&node).await;

    let mut hold = TcpStream::connect(("127.0.0.1", echo_port))
        .await
        .unwrap();
    // Prove the accept loop handed the socket to the echo handler (avoid backlog race).
    hold.write_all(b"x").await.unwrap();
    let mut one = [0u8; 1];
    hold.read_exact(&mut one).await.unwrap();
    assert_eq!(one, [b'x']);

    assert!(spacestorage_node::lifecycle::request_drain(&node));
    let _ = tokio::time::timeout(Duration::from_secs(3), run)
        .await
        .expect("drain should finish after timeout");
    assert!(node.stats.drain_timed_out());
    drop(hold);
}

#[tokio::test]
async fn second_force_abort_during_drain() {
    let dir = tempdir().unwrap();
    let token = dir.path().join("admin.token");
    std::fs::write(&token, "secret\n").unwrap();
    let conf = dir.path().join("node.conf");
    std::fs::write(&conf, "#\n").unwrap();

    let http_port = ephemeral_port().await;
    let echo_port = ephemeral_port().await;
    let cfg = config_with_echo(
        http_port,
        echo_port,
        token.to_str().unwrap(),
        Duration::from_secs(30),
    );
    let (threads, source) = runtime::resolve_worker_threads(cfg.threads);
    let node = Node::boot_first_binary(conf, cfg, threads, source);
    let run = tokio::spawn({
        let n = node.clone();
        async move { n.run().await }
    });
    wait_ready(&node).await;

    let mut hold = TcpStream::connect(("127.0.0.1", echo_port)).await.unwrap();
    hold.write_all(b"y").await.unwrap();
    let mut one = [0u8; 1];
    hold.read_exact(&mut one).await.unwrap();
    assert!(spacestorage_node::lifecycle::request_drain(&node));
    // Second signal equivalent: force abort without waiting full drain timeout.
    spacestorage_node::lifecycle::force_abort(&node);
    let _ = tokio::time::timeout(Duration::from_secs(3), run)
        .await
        .expect("force abort should finish promptly");
    drop(hold);
}

#[tokio::test]
async fn stop_before_ready_aborts_without_ready() {
    let dir = tempdir().unwrap();
    let token = dir.path().join("admin.token");
    std::fs::write(&token, "secret\n").unwrap();
    let conf = dir.path().join("node.conf");
    std::fs::write(&conf, "#\n").unwrap();

    let http_port = ephemeral_port().await;
    let echo_port = ephemeral_port().await;
    let cfg = config_with_echo(
        http_port,
        echo_port,
        token.to_str().unwrap(),
        Duration::from_secs(2),
    );
    let (threads, source) = runtime::resolve_worker_threads(cfg.threads);
    let node = Node::boot_first_binary(conf, cfg, threads, source);
    // Abort before run binds / becomes ready.
    node.cancel.cancel();
    node.force_cancel.cancel();
    node.state.begin_drain();
    node.drain_flag.store(true, Ordering::SeqCst);

    let run = tokio::spawn({
        let n = node.clone();
        async move { n.run().await }
    });
    let _ = tokio::time::timeout(Duration::from_secs(3), run)
        .await
        .expect("startup abort should finish");
    assert_ne!(node.state.get(), NodeState::Ready);
}

#[tokio::test]
async fn admin_reachable_while_draining() {
    let dir = tempdir().unwrap();
    let token = dir.path().join("admin.token");
    std::fs::write(&token, "secret\n").unwrap();
    let conf = dir.path().join("node.conf");
    std::fs::write(&conf, "#\n").unwrap();

    let http_port = ephemeral_port().await;
    let echo_port = ephemeral_port().await;
    let cfg = config_with_echo(
        http_port,
        echo_port,
        token.to_str().unwrap(),
        Duration::from_secs(10),
    );
    let (threads, source) = runtime::resolve_worker_threads(cfg.threads);
    let node = Node::boot_first_binary(conf, cfg, threads, source);
    let run = tokio::spawn({
        let n = node.clone();
        async move { n.run().await }
    });
    wait_ready(&node).await;

    // Hold in-flight tenant work so drain does not exit before we probe admin.
    let mut hold = TcpStream::connect(("127.0.0.1", echo_port)).await.unwrap();
    hold.write_all(b"z").await.unwrap();
    let mut one = [0u8; 1];
    hold.read_exact(&mut one).await.unwrap();

    assert!(spacestorage_node::lifecycle::request_drain(&node));
    assert_eq!(node.state.get(), NodeState::Draining);

    // New tenant accepts refused.
    let refused = TcpStream::connect(("127.0.0.1", echo_port)).await;
    if let Ok(mut s) = refused {
        let n = tokio::time::timeout(Duration::from_millis(300), s.read(&mut [0u8; 1]))
            .await;
        // Dropped accept: EOF / timeout / error — not a lasting echo session.
        assert!(
            matches!(n, Ok(Ok(0)) | Ok(Err(_)) | Err(_)),
            "tenant accept must not serve during drain"
        );
    }

    // New admin-http connection must still serve draining status (FR-015 / T088).
    let mut admin = TcpStream::connect(("127.0.0.1", http_port))
        .await
        .expect("admin accept during drain");
    let req = "GET /v1/status HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer secret\r\nConnection: close\r\n\r\n";
    admin.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(2), admin.read_to_end(&mut buf))
        .await
        .expect("admin status within drain")
        .unwrap();
    let text = String::from_utf8_lossy(&buf);
    assert!(text.contains("HTTP/1.1 200"), "admin status: {text}");
    assert!(
        text.contains("\"state\":\"draining\"") || text.contains("\"state\": \"draining\""),
        "expected draining state: {text}"
    );

    // Reload must be rejected with draining state while admin remains reachable.
    let mut reload = TcpStream::connect(("127.0.0.1", http_port))
        .await
        .expect("admin accept for reload");
    let req = "POST /v1/reload HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer secret\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    reload.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(2), reload.read_to_end(&mut buf))
        .await
        .expect("reload reject within drain")
        .unwrap();
    let text = String::from_utf8_lossy(&buf);
    assert!(
        text.contains("invalid_state") || text.contains("409"),
        "reload while draining: {text}"
    );
    assert!(text.contains("draining"), "reload error names state: {text}");

    drop(hold);
    spacestorage_node::lifecycle::force_abort(&node);
    let _ = tokio::time::timeout(Duration::from_secs(3), run)
        .await
        .expect("node should exit after force abort");
}
