//! Migrate live evacuate + anti-affinity (010 SC-001) — feature `migration-backup`.

#![cfg(feature = "migration-backup")]

use spacestorage_migrate::{
    AuthzContext, ContainerRef, JobService, MigratePolicy, MigrationSpec, QuotaView, Strategy,
};
use spacestorage_storage::{ContentStore, StorageMode};
use std::collections::HashMap;
use uuid::Uuid;

#[test]
fn live_evacuate_copies_kv() {
    let svc = JobService::new(true);
    let runner = svc.migrate_runner();
    let source_id = Uuid::now_v7();
    let mut rows = HashMap::new();
    rows.insert(b"a".to_vec(), b"1".to_vec());
    rows.insert(b"b".to_vec(), b"2".to_vec());
    let mut names = HashMap::new();
    names.insert(("acme".into(), "kv".into()), source_id);
    let mut content = ContentStore::default();
    content.register_mode(source_id, StorageMode::Persistent);
    for (k, v) in &rows {
        content.put(source_id, k.clone(), v.clone());
    }

    let body = serde_json::json!({
        "source": {"namespace":"acme","container":"kv"},
        "dest_nodes": ["node-b"],
        "policy": "copy"
    });
    let authz = AuthzContext::cluster_admin();
    let job = svc
        .start(
            spacestorage_migrate::JobKind::DataMigration,
            Uuid::nil(),
            Strategy::Live,
            body,
            &authz,
        )
        .unwrap();
    let spec = MigrationSpec {
        source: ContainerRef {
            namespace: "acme".into(),
            container: "kv".into(),
        },
        dest_namespace: None,
        dest_name: Some("kv".into()),
        policy: MigratePolicy::Copy,
        dest_nodes: Some(vec!["node-b".into()]),
        dest_drives: None,
        strategy: Strategy::Live,
        force_unsatisfiable: false,
    };
    // Dest name must be free for node evacuate into same ns with new placement —
    // use a distinct dest name for the incomplete→public publish.
    let mut spec = spec;
    spec.dest_name = Some("kv_b".into());
    let done = runner
        .run_live(
            job,
            &spec,
            source_id,
            &rows,
            10,
            &mut names,
            &mut content,
            QuotaView::default(),
            64,
        )
        .unwrap();
    assert_eq!(done.status, spacestorage_migrate::JobStatus::Completed);
    let dest_id = *names.get(&("acme".into(), "kv_b".into())).unwrap();
    assert_eq!(content.get(dest_id, b"a"), Some(b"1".as_slice()));
    assert_eq!(content.len(source_id), 2); // copy leaves source
}

#[test]
fn anti_affinity_refuses_before_copy() {
    let spec = MigrationSpec {
        source: ContainerRef {
            namespace: "acme".into(),
            container: "kv".into(),
        },
        dest_namespace: None,
        dest_name: None,
        policy: MigratePolicy::Copy,
        dest_nodes: Some(vec!["node-b".into()]),
        dest_drives: None,
        strategy: Strategy::Live,
        force_unsatisfiable: true,
    };
    let err = spec
        .validate(&AuthzContext::cluster_admin())
        .unwrap_err();
    assert_eq!(err.code(), "ConstraintUnsatisfiable");
}
