//! Namespace copy/move + name/quota (010 SC-002).

#![cfg(feature = "migration-backup")]

use spacestorage_authz::permission::Verb;
use spacestorage_migrate::{
    AuthzContext, ContainerRef, JobKind, JobService, JobStatus, MigratePolicy, MigrationSpec,
    QuotaView, Strategy,
};
use spacestorage_storage::{ContentStore, StorageMode};
use std::collections::HashMap;
use uuid::Uuid;

fn base_spec() -> MigrationSpec {
    MigrationSpec {
        source: ContainerRef {
            namespace: "acme".into(),
            container: "t".into(),
        },
        dest_namespace: Some("beta".into()),
        dest_name: Some("t".into()),
        policy: MigratePolicy::Copy,
        dest_nodes: None,
        dest_drives: None,
        strategy: Strategy::Live,
        force_unsatisfiable: false,
    }
}

#[test]
fn copy_leaves_source_move_drops() {
    let svc = JobService::new(true);
    let runner = svc.migrate_runner();
    let source_id = Uuid::now_v7();
    let mut rows = HashMap::new();
    rows.insert(b"k".to_vec(), b"v".to_vec());
    let mut names = HashMap::new();
    names.insert(("acme".into(), "t".into()), source_id);
    let mut content = ContentStore::default();
    content.register_mode(source_id, StorageMode::Persistent);
    content.put(source_id, b"k".to_vec(), b"v".to_vec());

    let authz = AuthzContext::cluster_admin();
    let body = serde_json::to_value(base_spec()).unwrap();
    let job = svc
        .start(JobKind::DataMigration, Uuid::nil(), Strategy::Live, body, &authz)
        .unwrap();
    let done = runner
        .run_live(
            job,
            &base_spec(),
            source_id,
            &rows,
            1,
            &mut names,
            &mut content,
            QuotaView::default(),
            8,
        )
        .unwrap();
    assert_eq!(done.status, JobStatus::Completed);
    assert!(names.contains_key(&("acme".into(), "t".into())));
    assert!(names.contains_key(&("beta".into(), "t".into())));

    let source2 = Uuid::now_v7();
    names.insert(("acme".into(), "t2".into()), source2);
    content.register_mode(source2, StorageMode::Persistent);
    content.put(source2, b"k".to_vec(), b"v".to_vec());
    let mut move_spec = base_spec();
    move_spec.source.container = "t2".into();
    move_spec.dest_name = Some("t2".into());
    move_spec.policy = MigratePolicy::Move;
    let body2 = serde_json::to_value(&move_spec).unwrap();
    let job2 = svc
        .start(JobKind::DataMigration, Uuid::nil(), Strategy::Live, body2, &authz)
        .unwrap();
    runner
        .run_live(
            job2,
            &move_spec,
            source2,
            &rows,
            1,
            &mut names,
            &mut content,
            QuotaView::default(),
            8,
        )
        .unwrap();
    assert_eq!(content.len(source2), 0);
}

#[test]
fn name_exists_and_quota_and_authz() {
    let spec = base_spec();
    let denied = AuthzContext {
        verbs: Verb::READ,
        dual_migrate: false,
    };
    assert_eq!(spec.validate(&denied).unwrap_err().code(), "AuthzDenied");

    let q = QuotaView {
        logical_usage: 100,
        soft_limit: Some(100),
    };
    assert_eq!(q.check_fits(1).unwrap_err().code(), "QuotaExceeded");
}
