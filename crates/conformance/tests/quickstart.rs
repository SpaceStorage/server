//! Quickstart command set (T051) — SC-001–SC-003 / SC-006 executable gate.
//!
//! Mirrors [specs/016-mvp-and-nongoals/quickstart.md](../../../specs/016-mvp-and-nongoals/quickstart.md)
//! against in-process nodes so the operator path stays CI-runnable without a
//! loopback binary install.

use spacestorage_compat::DialectProfile;
use spacestorage_conformance::{
    boot_one_node, boot_three_node, quorum_two_write, scrape_metrics, tcp_bound, validate_fixture,
};
use spacestorage_handler_postgresql::{FEATURE_NOT_SUPPORTED, execute_sql};
use spacestorage_handler_redis::{RedisReply, SessionState, dispatch};
use spacestorage_types::{L3Model, StorageModeChoice};
use std::sync::Arc;
use tempfile::tempdir;

#[test]
fn quickstart_step2_validate_rejects() {
    // quickstart §2 — unknown handler + omitted transport
    let cass = validate_fixture("invalid/unknown-handler-cassandra.conf").unwrap_err();
    assert!(cass.iter().any(|e| e.code == "entrypoint_unknown_handler"));
    let transport = validate_fixture("invalid/omitted-transport.conf").unwrap_err();
    assert!(transport.iter().any(|e| e.code == "transport_undeclared"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn quickstart_steps_3_to_7_in_process() {
    // §3 one-node start (write ONE) + metrics
    let dir = tempdir().unwrap();
    let lab = boot_one_node(dir.path()).await;
    assert_eq!(lab.write_quorum(), "ONE");
    assert!(tcp_bound(lab.ports.internode).await);
    assert!(tcp_bound(lab.ports.replication).await);
    let m = scrape_metrics(lab.ports.admin_http).await;
    assert!(m.contains("200") || m.contains("#"), "metrics: {m}");

    // §4 PostgreSQL smoke — BEGIN/COMMIT/ROLLBACK/COPY → 0A000 (dialect unit path)
    let cat = Arc::clone(&lab.node.catalog);
    for sql in ["BEGIN", "COMMIT", "ROLLBACK", "COPY t FROM STDIN"] {
        let err =
            execute_sql(DialectProfile::FirstBinary, &cat, "demo", sql).expect_err("must refuse");
        assert_eq!(err.sqlstate, FEATURE_NOT_SUPPORTED, "{sql} → {err:?}");
    }

    // §5 Redis MUST + HGET/JSON.GET error (SC-006)
    let mut s = SessionState::demo(Arc::clone(&lab.node.catalog));
    assert!(matches!(
        dispatch(&mut s, "AUTH", &["demo", "demo"]),
        RedisReply::Ok
    ));
    assert!(matches!(
        dispatch(&mut s, "SET", &["k", "v"]),
        RedisReply::Ok
    ));
    assert!(matches!(
        dispatch(&mut s, "GET", &["k"]),
        RedisReply::Bulk(Some(_))
    ));
    assert!(matches!(
        dispatch(&mut s, "HGET", &["k", "f"]),
        RedisReply::Error(_)
    ));
    assert!(matches!(
        dispatch(&mut s, "JSON.GET", &["k"]),
        RedisReply::Error(_)
    ));

    // §5b Document Store admin-create + canonical blob
    {
        let mut c = lab.node.catalog.write().expect("catalog");
        c.create(
            "demo",
            "docs",
            L3Model::DocumentStore,
            false,
            None,
            StorageModeChoice::Persistent,
        )
        .expect("admin-create Document Store");
    }
    s.container = "docs".into();
    assert!(matches!(
        dispatch(&mut s, "SET", &["d", r#"{"x":1}"#]),
        RedisReply::Ok
    ));
    assert!(matches!(
        dispatch(&mut s, "GET", &["d"]),
        RedisReply::Bulk(Some(_))
    ));
    assert!(matches!(
        dispatch(&mut s, "JSON.GET", &["d"]),
        RedisReply::Error(_)
    ));

    lab.shutdown().await;

    // §6–§7 three-node + kill-one write TWO
    let dir3 = tempdir().unwrap();
    let (a, b, c) = boot_three_node(dir3.path()).await;
    assert_eq!(a.write_quorum(), "TWO");
    let secret = a.join_secret_bytes().await;
    let replicas = vec![
        ("127.0.0.1".into(), a.ports.replication),
        ("127.0.0.1".into(), b.ports.replication),
        ("127.0.0.1".into(), c.ports.replication),
    ];
    assert!(
        quorum_two_write(&replicas, &secret, b"qs-1")
            .await
            .expect("TWO")
            >= 2
    );
    c.shutdown().await;
    let replicas2 = vec![
        ("127.0.0.1".into(), a.ports.replication),
        ("127.0.0.1".into(), b.ports.replication),
    ];
    assert!(
        quorum_two_write(&replicas2, &secret, b"qs-2")
            .await
            .expect("TWO after kill-one")
            >= 2
    );
    a.shutdown().await;
    b.shutdown().await;
}

/// Full v1 operator path: 3-node + Redis protocol + live Raft TCP + L0 later surface.
#[cfg(feature = "complete-product")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn quickstart_full_v1_three_node_raft_and_l0() {
    use openraft::BasicNode;
    use spacestorage_controlplane::{
        bootstrap_multi_voter, start_raft_peered_with_secret, PeerRaftRegistry, RegistryRaftHandler,
    };
    use spacestorage_internode::{RaftPeerHandler, RaftRpcKind};
    use spacestorage_types::StorageModeChoice;
    use std::collections::BTreeMap;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use uuid::Uuid;

    let dir3 = tempdir().unwrap();
    let (a, b, c) = boot_three_node(dir3.path()).await;
    assert_eq!(a.write_quorum(), "TWO");

    // One HC-capable protocol path (Redis is always on; Cassandra available under complete-product).
    let mut s = SessionState::demo(Arc::clone(&a.node.catalog));
    assert!(matches!(
        dispatch(&mut s, "AUTH", &["demo", "demo"]),
        RedisReply::Ok
    ));
    assert!(matches!(
        dispatch(&mut s, "SET", &["full-v1", "ok"]),
        RedisReply::Ok
    ));

    // Later surface on the live node.
    let id = a
        .node
        .later
        .create_l0("acme", "ht", "hash_table", StorageModeChoice::Memory)
        .expect("l0 create");
    assert!(!id.is_nil());

    // Live multi-node Raft over real TCP (control-plane fabric path).
    let registry = Arc::new(PeerRaftRegistry::new());
    let secret = Arc::new(a.join_secret_bytes().await);
    if secret.is_empty() {
        // Lab fixtures may use empty fabric secret; still prove dial path with shared bytes.
    }
    let secret = if secret.is_empty() {
        Arc::new(b"full-v1-raft-secret-0123456789".to_vec())
    } else {
        secret
    };

    async fn spawn_tcp(
        registry: Arc<PeerRaftRegistry>,
        secret: Arc<Vec<u8>>,
    ) -> (Uuid, String, spacestorage_controlplane::ControlRaft, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let dial = format!("{}:{}", addr.ip(), addr.port());
        let id = Uuid::now_v7();
        let raft = start_raft_peered_with_secret(id, Arc::clone(&registry), Arc::clone(&secret))
            .await
            .unwrap();
        registry.register(id, raft.clone());
        let handler = Arc::new(RegistryRaftHandler {
            registry: Arc::clone(&registry),
            local_id: id,
        });
        let secret_accept = Arc::clone(&secret);
        let accept = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let h = Arc::clone(&handler);
                let s = Arc::clone(&secret_accept);
                tokio::spawn(async move {
                    use spacestorage_internode::{decode_frame, encode_frame, registry as reg};
                    let mut buf = Vec::new();
                    let mut tmp = [0u8; 8192];
                    let mut authed = false;
                    loop {
                        let n = match stream.read(&mut tmp).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => n,
                        };
                        buf.extend_from_slice(&tmp[..n]);
                        while let Ok((frame, consumed)) = decode_frame(&buf) {
                            buf.drain(..consumed);
                            if !authed {
                                if frame.payload.as_slice() != s.as_slice() {
                                    return;
                                }
                                authed = true;
                                let _ = stream
                                    .write_all(&encode_frame(reg::MSG_ACK, br#"{"ok":true}"#))
                                    .await;
                                continue;
                            }
                            let Some(kind) = RaftRpcKind::from_msg_type(frame.msg_type) else {
                                continue;
                            };
                            if let Ok(body) = h.handle_raft(kind, &frame.payload).await {
                                let _ = stream
                                    .write_all(&encode_frame(reg::MSG_ACK, &body))
                                    .await;
                            }
                        }
                    }
                });
            }
        });
        (id, dial, raft, accept)
    }

    let n1 = spawn_tcp(Arc::clone(&registry), Arc::clone(&secret)).await;
    let n2 = spawn_tcp(Arc::clone(&registry), Arc::clone(&secret)).await;
    let n3 = spawn_tcp(Arc::clone(&registry), Arc::clone(&secret)).await;
    let mut members = BTreeMap::new();
    members.insert(n1.0, BasicNode::new(n1.1.clone()));
    members.insert(n2.0, BasicNode::new(n2.1.clone()));
    members.insert(n3.0, BasicNode::new(n3.1.clone()));
    bootstrap_multi_voter(&n1.2, members).await.unwrap();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let mut leader = None;
    while tokio::time::Instant::now() < deadline {
        for (id, _, raft, _) in [&n1, &n2, &n3] {
            if raft.is_leader() {
                leader = Some(*id);
                break;
            }
        }
        if leader.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert!(leader.is_some(), "full v1 raft elected a leader over TCP");

    n1.3.abort();
    n2.3.abort();
    n3.3.abort();
    a.shutdown().await;
    b.shutdown().await;
    c.shutdown().await;
}
