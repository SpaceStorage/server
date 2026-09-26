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
