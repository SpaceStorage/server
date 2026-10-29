//! `007` builtin roles in cluster-applied state (SC-008).

use spacestorage_admin_proto::AdminOp;
use spacestorage_authz::Verb;
use spacestorage_conformance::{
    LAB_ADMIN_TOKEN, admin_tcp_json, boot_tenancy_one_node, reboot_lab,
};
use spacestorage_tenancy::roles::{Role, ScopeKind, role_put};
use tempfile::tempdir;

async fn role_names(lab: &spacestorage_conformance::LabNode) -> Vec<String> {
    let roles = admin_tcp_json(lab.ports.admin, LAB_ADMIN_TOKEN, AdminOp::RoleList).await;
    roles["roles"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| row["name"].as_str().map(String::from))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn builtin_roles_survive_restart() {
    let dir = tempdir().unwrap();
    let lab = boot_tenancy_one_node(dir.path()).await;
    let first = role_names(&lab).await;
    assert!(first.contains(&"admin".into()));
    assert!(first.contains(&"replication".into()));

    let lab2 = reboot_lab(lab).await;
    let second = role_names(&lab2).await;
    assert_eq!(first, second);
    lab2.shutdown().await;
}

#[test]
fn custom_role_put_without_slice7_is_refused() {
    // A workspace build that also compiles spacestoraged turns the feature on
    // for this crate. That binary is the slice-7 server; this check is the off build.
    if spacestorage_tenancy::TENANCY_QUOTAS {
        return;
    }
    let err = role_put(
        Verb::CLUSTER_ADMIN.bits(),
        Role {
            name: "reader".into(),
            scope_kind: ScopeKind::Namespace,
            verbs: Verb::READ.bits(),
            container_ids: Vec::new(),
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("Slice7Required"), "{err}");
}
