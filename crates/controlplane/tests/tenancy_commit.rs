use spacestorage_controlplane::{ControlPlane, Error};
use spacestorage_tenancy::TenancyOp;
use tempfile::tempdir;
use uuid::Uuid;

#[tokio::test]
async fn tenancy_is_visible_only_after_cluster_commit() {
    let dir = tempdir().unwrap();
    let node = Uuid::now_v7();
    let cp = ControlPlane::bootstrap(dir.path(), node, "n1", Uuid::now_v7(), "c")
        .await
        .unwrap();

    cp.append_tenancy(TenancyOp::NamespaceCreate {
        name: "acme".into(),
    })
    .await
    .unwrap();

    let record = cp.tenancy_namespace_by_name("acme").unwrap();
    assert_eq!(record.name, "acme");
    assert!(cp.cluster_snapshot().namespaces.contains_key(&record.id));
}

#[tokio::test]
async fn tenancy_write_refuses_without_cluster_majority() {
    let dir = tempdir().unwrap();
    let node = Uuid::now_v7();
    let cp = ControlPlane::bootstrap(dir.path(), node, "n1", Uuid::now_v7(), "c")
        .await
        .unwrap();
    {
        let mut group = cp.cluster_group.lock().await;
        let group = group.as_mut().unwrap();
        group.voter_set.voters.insert(Uuid::now_v7());
        group.voter_set.voters.insert(Uuid::now_v7());
    }

    let error = cp
        .append_tenancy(TenancyOp::NamespaceCreate {
            name: "minority".into(),
        })
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Minority { .. }));
    assert!(cp.tenancy_namespace_by_name("minority").is_none());
}
