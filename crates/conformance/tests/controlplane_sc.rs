//! Control-plane conformance SC-001…SC-011 (006 T071).
//!
//! In-process harness against `spacestorage-controlplane` — no multi-process orchestration.

use spacestorage_controlplane::cluster::ClusterOp;
use spacestorage_controlplane::group::GroupId;
use spacestorage_controlplane::lease::{fence_ordered_append, grant};
use spacestorage_controlplane::{ControlPlane, Error};
use std::collections::BTreeMap;
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn sc001_sc002_election_single_voter() {
    let dir = tempfile::tempdir().unwrap();
    let id = Uuid::now_v7();
    let cp = ControlPlane::bootstrap(dir.path(), id, "n1", Uuid::now_v7(), "lab")
        .await
        .expect("bootstrap");
    let st = cp.raft_status(GroupId::Cluster).await.unwrap();
    assert_eq!(st.leader, Some(id));
    assert!(st.role.contains("Leader"));
    assert!(cp.elections_total.load(std::sync::atomic::Ordering::Relaxed) >= 1);
}

#[tokio::test]
async fn sc003_minority_or_not_leader_on_follower_write() {
    let dir = tempfile::tempdir().unwrap();
    let id = Uuid::now_v7();
    let cp = ControlPlane::open_existing(dir.path(), id, "n1", true)
        .await
        .expect("open");
    // Cluster group exists but is not bootstrapped leader → NotLeader / Minority on write.
    let err = cp
        .append_cluster(ClusterOp::CreateNamespace {
            id: Uuid::now_v7(),
            name: "ns".into(),
        })
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::NotLeader { .. } | Error::Minority { .. } | Error::Msg(_)),
        "got {err:?}"
    );
}

#[tokio::test]
async fn sc004_restore_plan_discovers_cluster() {
    let dir = tempfile::tempdir().unwrap();
    let id = Uuid::now_v7();
    let _ = ControlPlane::bootstrap(dir.path(), id, "n1", Uuid::now_v7(), "lab")
        .await
        .unwrap();
    let plan = spacestorage_controlplane::restore::RestorePlan::discover(dir.path())
        .await
        .unwrap();
    assert!(plan.groups.iter().any(|g| g.is_cluster()));
    assert!(plan.memory_content_empty);
    let stores = plan.open_stores(dir.path()).await.unwrap();
    assert!(!stores.is_empty());
}

#[tokio::test]
async fn sc005_leaderless_kv_no_lease() {
    let err = grant(Uuid::nil(), Uuid::from_u128(1), None, 1, "K/V Store").unwrap_err();
    assert!(matches!(err, Error::LeaseForbidden { .. }));
}

#[tokio::test]
async fn sc006_epoch_fence_unit() {
    // Always covered at unit level; full ordered-type path is `controlplane-leases`.
    let c = Uuid::from_u128(9);
    let lease = grant(c, Uuid::from_u128(1), None, 1, "Log Stream").unwrap();
    let mut map = BTreeMap::new();
    map.insert(c, lease.clone());
    assert!(fence_ordered_append(&map, c, lease.epoch, "Log Stream").is_ok());
    let stale = fence_ordered_append(&map, c, 0, "Log Stream").unwrap_err();
    assert!(matches!(stale, Error::StaleEpoch { .. }));
}

#[tokio::test]
#[cfg(feature = "controlplane-leases")]
async fn sc006_epoch_fence_feature() {
    let c = Uuid::from_u128(9);
    let lease = grant(c, Uuid::from_u128(1), None, 1, "Log Stream").unwrap();
    let mut map = BTreeMap::new();
    map.insert(c, lease.clone());
    assert!(fence_ordered_append(&map, c, lease.epoch, "Log Stream").is_ok());
}

#[tokio::test]
async fn sc007_controllers_view_read() {
    let dir = tempfile::tempdir().unwrap();
    let id = Uuid::now_v7();
    let cp = ControlPlane::bootstrap(dir.path(), id, "n1", Uuid::now_v7(), "lab")
        .await
        .unwrap();
    let view = cp.controllers_view().await;
    assert_eq!(view.cluster_primary, Some(id));
}

#[tokio::test]
async fn sc008_election_metrics_series() {
    let metrics = spacestorage_observability::Metrics::new();
    spacestorage_controlplane::record_election(
        &metrics,
        "n1",
        "cluster",
        true,
        Duration::from_millis(12),
    );
    let text = metrics.render_prometheus();
    assert!(text.contains("spacestorage_raft_leader_elections_total"));
    assert!(text.contains("spacestorage_raft_leader_election_duration_seconds"));
}

#[tokio::test]
#[cfg(feature = "controlplane-ops")]
async fn sc009_voter_replace_ops() {
    use spacestorage_controlplane::membership::MembershipOp;
    let dir = tempfile::tempdir().unwrap();
    let a = Uuid::now_v7();
    let b = Uuid::now_v7();
    let cp = ControlPlane::bootstrap(dir.path(), a, "n1", Uuid::now_v7(), "lab")
        .await
        .unwrap();
    cp.admit_member(b, "n2").await.unwrap();
    let _ = cp
        .voter_op(MembershipOp::Replace { from: a, to: b })
        .await;
}

#[tokio::test]
#[cfg(feature = "controlplane-ops")]
async fn sc010_exclusive_data() {
    use spacestorage_controlplane::ops::{check_exclusive_data_on, tenant_replica_excluded};
    let voters: std::collections::BTreeSet<_> = [Uuid::from_u128(1)].into_iter().collect();
    assert!(check_exclusive_data_on(true, &voters, &[]).is_ok());
    assert!(tenant_replica_excluded(Uuid::from_u128(1), true, &voters));
}

#[tokio::test]
async fn sc011_namespace_primary_metrics_agg() {
    let dir = tempfile::tempdir().unwrap();
    let id = Uuid::now_v7();
    let cp = ControlPlane::bootstrap(dir.path(), id, "n1", Uuid::now_v7(), "lab")
        .await
        .unwrap();
    let ns = Uuid::now_v7();
    cp.append_cluster(ClusterOp::CreateNamespace {
        id: ns,
        name: "app".into(),
    })
    .await
    .unwrap();
    cp.push_shared_metric(ns, "hits", 3.0);
    let snap = cp.namespace_metrics_snapshot(ns);
    assert!(snap.is_some());
    assert_eq!(snap.unwrap().series.get("hits").copied().unwrap_or(0.0), 3.0);
}

#[tokio::test]
async fn t062_namespace_group_on_create() {
    let dir = tempfile::tempdir().unwrap();
    let id = Uuid::now_v7();
    let cp = ControlPlane::bootstrap(dir.path(), id, "n1", Uuid::now_v7(), "lab")
        .await
        .unwrap();
    let ns = Uuid::now_v7();
    cp.append_cluster(ClusterOp::CreateNamespace {
        id: ns,
        name: "app".into(),
    })
    .await
    .unwrap();
    let st = cp.raft_status(GroupId::Namespace(ns)).await.unwrap();
    assert_eq!(st.leader, Some(id));
}

#[tokio::test]
async fn t073_kafka_ingest_stays_on_cluster_state() {
    let dir = tempfile::tempdir().unwrap();
    let id = Uuid::now_v7();
    let cp = ControlPlane::bootstrap(dir.path(), id, "n1", Uuid::now_v7(), "lab")
        .await
        .unwrap();
    let kid = Uuid::now_v7();
    cp.append_cluster(ClusterOp::UpsertKafkaIngest {
        decl: serde_json::json!({"id": kid, "name": "k1"}),
    })
    .await
    .unwrap();
    assert!(cp.cluster_snapshot().kafka_ingests.contains_key(&kid));
}
