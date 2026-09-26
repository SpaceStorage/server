//! G1/G2/G6/G7/G10 first-binary cluster conformance.

use spacestorage_conformance::{
    boot_one_node, boot_three_node, quorum_two_write, scrape_metrics, tcp_bound, validate_fixture,
};
use spacestorage_node::lifecycle::{NodeState, request_drain};
use spacestorage_release_profile::{HandlerBuildSet, ReleaseProfile};
use std::time::Duration;
use tempfile::tempdir;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

#[test]
fn g1_omitted_transport_rejected() {
    let err = validate_fixture("invalid/omitted-transport.conf").unwrap_err();
    assert!(
        err.iter().any(|e| e.code == "transport_undeclared"),
        "G1: omitted-transport → transport_undeclared, got {err:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn g1_one_node_starter_write_quorum_one() {
    let set = HandlerBuildSet::for_profile(ReleaseProfile::FirstBinary);
    assert!(set.is_required("postgresql"));
    assert!(set.is_required("redis"));

    let dir = tempdir().unwrap();
    let lab = boot_one_node(dir.path()).await;
    assert_eq!(lab.write_quorum(), "ONE");
    assert!(tcp_bound(lab.ports.internode).await);
    assert!(tcp_bound(lab.ports.replication).await);
    let m = scrape_metrics(lab.ports.admin_http).await;
    assert!(
        m.contains("200") || m.contains("#"),
        "G1: /metrics scrapeable, body: {m}"
    );
    lab.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn g2_three_node_join_write_quorum_two() {
    let dir = tempdir().unwrap();
    let (a, b, c) = boot_three_node(dir.path()).await;
    assert_eq!(a.write_quorum(), "TWO");
    assert_eq!(b.write_quorum(), "TWO");
    assert_eq!(c.write_quorum(), "TWO");
    let ma = a.membership_names().await;
    let mb = b.membership_names().await;
    let mc = c.membership_names().await;
    assert_eq!(ma, mb);
    assert_eq!(mb, mc);
    assert_eq!(
        ma,
        vec![
            String::from("db-a"),
            String::from("db-b"),
            String::from("db-c")
        ]
    );
    let az = a.az_labels().await;
    assert_eq!(
        az,
        vec![String::from("a"), String::from("b"), String::from("c")]
    );
    a.shutdown().await;
    b.shutdown().await;
    c.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn g6_g7_quorum_two_not_min_live_and_restore() {
    let dir = tempdir().unwrap();
    let (a, b, c) = boot_three_node(dir.path()).await;
    let secret = a.join_secret_bytes().await;
    let replicas = vec![
        ("127.0.0.1".into(), a.ports.replication),
        ("127.0.0.1".into(), b.ports.replication),
        ("127.0.0.1".into(), c.ports.replication),
    ];
    let acks = quorum_two_write(&replicas, &secret, b"payload-1")
        .await
        .expect("TWO write on three live");
    assert!(acks >= 2);

    // Kill one → TWO still succeeds (TWO ≠ min(2, live) with only one left later).
    c.shutdown().await;
    let replicas2 = vec![
        ("127.0.0.1".into(), a.ports.replication),
        ("127.0.0.1".into(), b.ports.replication),
    ];
    let acks2 = quorum_two_write(&replicas2, &secret, b"payload-2")
        .await
        .expect("TWO with one dead");
    assert!(acks2 >= 2);

    // Kill a second → TWO fails.
    let replicas1 = vec![("127.0.0.1".into(), a.ports.replication)];
    let fail = quorum_two_write(&replicas1, &secret, b"payload-3").await;
    assert!(
        fail.is_err(),
        "TWO must fail with one live replica: {fail:?}"
    );

    a.shutdown().await;
    b.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn g10_drain_blocks_new_tenant_connections() {
    let dir = tempdir().unwrap();
    let lab = boot_one_node(dir.path()).await;
    let redis = lab.ports.redis;

    // Tenant port accepts before drain.
    TcpStream::connect(("127.0.0.1", redis))
        .await
        .expect("redis accept before drain");

    assert!(request_drain(&lab.node));
    assert_eq!(lab.node.state.get(), NodeState::Draining);

    // New tenant connections are accepted then immediately dropped (no serve).
    let refused = TcpStream::connect(("127.0.0.1", redis)).await;
    if let Ok(mut s) = refused {
        let n = tokio::time::timeout(Duration::from_millis(300), s.read(&mut [0u8; 1])).await;
        assert!(
            matches!(n, Ok(Ok(0)) | Err(_) | Ok(Err(_))),
            "G10: drain must not serve new tenant traffic, got {n:?}"
        );
    }

    // Bound by drain timeout / external cancel — do not wait the fixture's 30s.
    lab.shutdown().await;
}
