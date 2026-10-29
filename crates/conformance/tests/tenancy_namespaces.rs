//! `007` namespace registry on the first binary (SC-001, SC-009, SC-010).

use spacestorage_admin_proto::AdminOp;
use spacestorage_conformance::{LAB_ADMIN_TOKEN, admin_tcp_json, boot_tenancy_one_node};
use spacestorage_handler_postgresql::refuse_database_open;
use tempfile::tempdir;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn starter_acme_and_cluster_crud() {
    let dir = tempdir().unwrap();
    let lab = boot_tenancy_one_node(dir.path()).await;

    let list = admin_tcp_json(lab.ports.admin, LAB_ADMIN_TOKEN, AdminOp::NamespaceList).await;
    let names: Vec<_> = list["namespaces"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| row["name"].as_str())
        .collect();
    assert!(names.iter().any(|n| *n == "acme"), "starter acme: {list}");

    admin_tcp_json(
        lab.ports.admin,
        LAB_ADMIN_TOKEN,
        AdminOp::NamespaceCreate {
            name: "otherns".into(),
        },
    )
    .await;

    let list2 = admin_tcp_json(lab.ports.admin, LAB_ADMIN_TOKEN, AdminOp::NamespaceList).await;
    let names2: Vec<_> = list2["namespaces"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| row["name"].as_str())
        .collect();
    assert!(names2.contains(&"acme"));
    assert!(names2.contains(&"otherns"));

    admin_tcp_json(
        lab.ports.admin,
        LAB_ADMIN_TOKEN,
        AdminOp::NamespaceRename {
            name: "otherns".into(),
            new_name: "otherns2".into(),
        },
    )
    .await;

    admin_tcp_json(
        lab.ports.admin,
        LAB_ADMIN_TOKEN,
        AdminOp::NamespaceDelete {
            name: "otherns2".into(),
            cascade: false,
        },
    )
    .await;

    lab.shutdown().await;
}

#[test]
fn tenant_cannot_open_cross_namespace_database() {
    assert!(refuse_database_open("demo", Some("acme"), "acme").is_ok());
    assert!(refuse_database_open("demo", Some("acme"), "other").is_err());
    assert!(refuse_database_open("admin", None, "other").is_ok());
}
