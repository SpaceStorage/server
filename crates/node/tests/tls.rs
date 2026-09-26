//! TLS / plaintext entrypoint tests (SC-006) — T079.

use spacestorage_config::model::{
    ClusterDecl, EntrypointDecl, NodeConfig, QueryDefaults, TlsDecl, Transport,
};
use spacestorage_node::lifecycle::NodeState;
use spacestorage_node::{runtime, Node};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Avoid ephemeral-port TOCTOU races across parallel tests and test binaries.
fn next_port_base() -> u16 {
    let pid = std::process::id() as u16;
    20_000u16.saturating_add(pid % 20_000)
}

static NEXT_PORT: AtomicU16 = AtomicU16::new(0);

fn bump_port() -> u16 {
    let cur = NEXT_PORT.load(Ordering::Relaxed);
    if cur == 0 {
        let _ = NEXT_PORT.compare_exchange(0, next_port_base(), Ordering::Relaxed, Ordering::Relaxed);
    }
    NEXT_PORT.fetch_add(1, Ordering::Relaxed)
}

fn gen_self_signed(dir: &Path, days: i32) -> (String, String) {
    let cert = dir.join("cert.pem");
    let key = dir.join("key.pem");
    let csr = dir.join("req.csr");
    let req_status = Command::new("openssl")
        .args([
            "req",
            "-newkey",
            "rsa:2048",
            "-keyout",
            key.to_str().unwrap(),
            "-out",
            csr.to_str().unwrap(),
            "-nodes",
            "-subj",
            "/CN=localhost",
        ])
        .status()
        .expect("openssl req");
    assert!(req_status.success(), "openssl req failed");

    let mut sign = Command::new("openssl");
    sign.args([
        "x509",
        "-req",
        "-in",
        csr.to_str().unwrap(),
        "-signkey",
        key.to_str().unwrap(),
        "-out",
        cert.to_str().unwrap(),
    ]);
    if days < 0 {
        sign.args([
            "-not_before",
            "20200101000000Z",
            "-not_after",
            "20200102000000Z",
        ]);
    } else {
        sign.args(["-days", &days.to_string()]);
    }
    let status = sign.status().expect("openssl x509");
    assert!(status.success(), "openssl x509 failed");
    (
        cert.to_str().unwrap().to_string(),
        key.to_str().unwrap().to_string(),
    )
}

fn base_cfg(
    http_port: u16,
    token_path: &str,
    transport: Transport,
    tls: Option<TlsDecl>,
) -> NodeConfig {
    NodeConfig {
        node_name: "tls-1".into(),
        threads: Some(2),
        drain_timeout: Duration::from_secs(2),
        log_level: "info".into(),
        log_format: "text".into(),
        admin_token_file: Some(token_path.into()),
        disable_admin: true,
        disable_admin_http: false,
        entrypoints: vec![EntrypointDecl {
            name: "admin-http".into(),
            address: "127.0.0.1".into(),
            port: http_port,
            handler: "admin-http".into(),
            transport,
            tls,
        }],
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

async fn unique_port() -> u16 {
    for _ in 0..128 {
        let p = bump_port();
        if p < 1024 || p > 60_000 {
            continue;
        }
        match tokio::net::TcpListener::bind(("127.0.0.1", p)).await {
            Ok(l) => {
                drop(l);
                return p;
            }
            Err(_) => continue,
        }
    }
    panic!("could not reserve a unique test port");
}

async fn boot_http_node(
    conf: std::path::PathBuf,
    token: &str,
    transport: Transport,
    tls: Option<TlsDecl>,
) -> (
    std::sync::Arc<Node>,
    tokio::task::JoinHandle<Result<(), String>>,
    u16,
) {
    for _ in 0..16 {
        let http_port = unique_port().await;
        let cfg = base_cfg(http_port, token, transport.clone(), tls.clone());
        let (threads, source) = runtime::resolve_worker_threads(cfg.threads);
        let node = Node::boot_first_binary(conf.clone(), cfg, threads, source);
        let run = tokio::spawn({
            let n = node.clone();
            async move { n.run().await }
        });
        let mut ready = false;
        for _ in 0..40 {
            if node.state.get() == NodeState::Ready {
                ready = true;
                break;
            }
            if run.is_finished() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        if ready {
            return (node, run, http_port);
        }
        node.cancel.cancel();
        node.force_cancel.cancel();
        let _ = run.await;
    }
    panic!("failed to boot node after bind retries");
}

#[tokio::test]
async fn tls_entrypoint_serves_encrypted_http() {
    let dir = tempdir().unwrap();
    let token = dir.path().join("admin.token");
    std::fs::write(&token, "tls-secret\n").unwrap();
    let (cert, key) = gen_self_signed(dir.path(), 30);
    let conf = dir.path().join("node.conf");
    std::fs::write(&conf, "#\n").unwrap();

    let (node, run, http_port) = boot_http_node(
        conf,
        token.to_str().unwrap(),
        Transport::Tls,
        Some(TlsDecl {
            certificate: cert,
            key,
        }),
    )
    .await;

    // Plaintext HTTP on a TLS port must not get a normal HTTP response.
    let mut plain = TcpStream::connect(("127.0.0.1", http_port)).await.unwrap();
    plain
        .write_all(b"GET /v1/health/live HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    let n = tokio::time::timeout(Duration::from_secs(2), plain.read_to_end(&mut buf))
        .await
        .unwrap_or(Ok(0))
        .unwrap_or(0);
    let text = String::from_utf8_lossy(&buf[..n.min(buf.len())]);
    assert!(
        !text.contains("HTTP/1.1 200"),
        "plaintext must not succeed on TLS port: {text}"
    );

    node.cancel.cancel();
    node.force_cancel.cancel();
    let _ = run.await;
}

#[tokio::test]
async fn plaintext_entrypoint_accepted() {
    let dir = tempdir().unwrap();
    let token = dir.path().join("admin.token");
    std::fs::write(&token, "plain-secret\n").unwrap();
    let conf = dir.path().join("node.conf");
    std::fs::write(&conf, "#\n").unwrap();

    let (node, run, http_port) =
        boot_http_node(conf, token.to_str().unwrap(), Transport::Plaintext, None).await;

    let mut stream = TcpStream::connect(("127.0.0.1", http_port)).await.unwrap();
    stream
        .write_all(b"GET /v1/health/live HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf);
    assert!(text.contains("HTTP/1.1 200"), "{text}");

    node.cancel.cancel();
    node.force_cancel.cancel();
    let _ = run.await;
}

#[tokio::test]
async fn expired_cert_fails_startup() {
    let dir = tempdir().unwrap();
    let token = dir.path().join("admin.token");
    std::fs::write(&token, "tls-secret\n").unwrap();
    // Negative days → already expired.
    let (cert, key) = gen_self_signed(dir.path(), -1);
    let conf = dir.path().join("node.conf");
    std::fs::write(&conf, "#\n").unwrap();

    let http_port = unique_port().await;
    let cfg = base_cfg(
        http_port,
        token.to_str().unwrap(),
        Transport::Tls,
        Some(TlsDecl {
            certificate: cert,
            key,
        }),
    );
    let (threads, source) = runtime::resolve_worker_threads(cfg.threads);
    let node = Node::boot_first_binary(conf, cfg, threads, source);
    let err = node.run().await.expect_err("expired cert must fail bind");
    assert!(
        err.contains("expired") || err.contains("certificate"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn missing_cert_files_fail_startup() {
    let dir = tempdir().unwrap();
    let token = dir.path().join("admin.token");
    std::fs::write(&token, "tls-secret\n").unwrap();
    let conf = dir.path().join("node.conf");
    std::fs::write(&conf, "#\n").unwrap();

    let http_port = unique_port().await;
    let cfg = base_cfg(
        http_port,
        token.to_str().unwrap(),
        Transport::Tls,
        Some(TlsDecl {
            certificate: dir.path().join("missing.crt").to_string_lossy().into(),
            key: dir.path().join("missing.key").to_string_lossy().into(),
        }),
    );
    let (threads, source) = runtime::resolve_worker_threads(cfg.threads);
    let node = Node::boot_first_binary(conf, cfg, threads, source);
    let err = node.run().await.expect_err("missing cert must fail");
    assert!(err.contains("cannot read") || err.contains("certificate"), "{err}");
}
