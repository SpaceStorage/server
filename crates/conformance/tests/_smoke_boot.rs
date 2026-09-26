use spacestorage_conformance::{
    boot_one_node, boot_three_node, quorum_two_write, scrape_metrics, tcp_bound,
};
use tempfile::tempdir;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn smoke_boot_one() {
    let dir = tempdir().unwrap();
    let lab = boot_one_node(dir.path()).await;
    assert_eq!(lab.write_quorum(), "ONE");
    assert!(tcp_bound(lab.ports.internode).await);
    assert!(tcp_bound(lab.ports.replication).await);
    let m = scrape_metrics(lab.ports.admin_http).await;
    assert!(m.contains("200") || m.contains("#"), "metrics body: {m}");
    println!("OK one-node");
    lab.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn smoke_boot_three() {
    let dir = tempdir().unwrap();
    let (a, b, c) = boot_three_node(dir.path()).await;
    assert_eq!(a.write_quorum(), "TWO");
    assert_eq!(b.write_quorum(), "TWO");
    assert_eq!(c.write_quorum(), "TWO");
    let ma = a.membership_names().await;
    let mb = b.membership_names().await;
    let mc = c.membership_names().await;
    println!("members a={ma:?} b={mb:?} c={mc:?}");
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
    println!("az={az:?}");
    assert_eq!(
        az,
        vec![String::from("a"), String::from("b"), String::from("c")]
    );
    let secret = a.join_secret_bytes().await;
    let replicas = vec![
        ("127.0.0.1".into(), a.ports.replication),
        ("127.0.0.1".into(), b.ports.replication),
        ("127.0.0.1".into(), c.ports.replication),
    ];
    let acks = quorum_two_write(&replicas, &secret, b"payload-1")
        .await
        .expect("TWO write");
    println!("durable_acks={acks}");
    assert!(acks >= 2);
    // kill c
    c.shutdown().await;
    let replicas2 = vec![
        ("127.0.0.1".into(), a.ports.replication),
        ("127.0.0.1".into(), b.ports.replication),
    ];
    let acks2 = quorum_two_write(&replicas2, &secret, b"payload-2")
        .await
        .expect("TWO with one dead");
    println!("after kill c durable_acks={acks2}");
    assert!(acks2 >= 2);
    let replicas1 = vec![("127.0.0.1".into(), a.ports.replication)];
    let fail = quorum_two_write(&replicas1, &secret, b"payload-3").await;
    println!("after kill b+c: {fail:?}");
    assert!(fail.is_err());
    a.shutdown().await;
    b.shutdown().await;
}
