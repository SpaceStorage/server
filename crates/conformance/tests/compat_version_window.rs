//! N/N+1 compat_version_window mixed fixture (full v1 Track 2).

#![cfg(feature = "complete-product")]

use spacestorage_compat::{evaluate_peer_join, ProductVersion, UpgradeJoin};
use spacestorage_membership::{JoinAck, JoinRequest, MembershipService, MembershipStartOpts};
use std::collections::BTreeMap;
use tempfile::tempdir;
use uuid::Uuid;

#[test]
fn compat_version_window_accepts_n_plus_1_refuses_n_plus_2() {
    assert_eq!(
        evaluate_peer_join(ProductVersion(1), ProductVersion(2)),
        UpgradeJoin::Accept
    );
    assert_eq!(
        evaluate_peer_join(ProductVersion(1), ProductVersion(3)),
        UpgradeJoin::Refuse { local: 1, peer: 3 }
    );
}

#[tokio::test]
async fn membership_join_refuses_product_version_window() {
    let dir = tempdir().unwrap();
    let svc = MembershipService::new(MembershipStartOpts {
        data_dir: dir.path().to_path_buf(),
        node_name: "n1".into(),
        cluster_name: "c".into(),
        quorum_domain: "lab".into(),
        token_file: None,
        bootstrap: true,
        join: false,
        foreign_seeds: false,
        topology_ladder: vec!["az".into()],
        labels: BTreeMap::new(),
    });
    svc.on_start().await.unwrap();
    let secret = svc.accepted_join_secret().unwrap_or_default();
    let req = JoinRequest {
        node_id: Uuid::now_v7(),
        node_name: "n2".into(),
        labels: BTreeMap::from([("az".into(), "a".into())]),
        internodes_address: "127.0.0.1:7000".into(),
        quorum_domain: "lab".into(),
        presented_secret: secret,
        join_token: None,
        replace_of: None,
        product_version: 3, // N+2 when local is 1
    };
    let ack = svc.apply_join(req).unwrap();
    assert!(matches!(
        ack,
        JoinAck::Refused { code } if code == "product_version_window"
    ));
}
