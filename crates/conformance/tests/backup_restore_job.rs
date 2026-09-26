//! Backup/restore job orchestration over 013 (010 SC-005).

#![cfg(feature = "migration-backup")]

use spacestorage_backup::ContainerSnapshotMeta;
use spacestorage_migrate::{
    AuthzContext, BackupOrchestrator, BackupScope, BackupSpec, JobKind, JobRecord, JobService,
    RestoreSpec, Strategy,
};
use spacestorage_storage::{StorageEngine, StorageMode, WalMetrics};
use std::sync::Arc;
use tempfile::tempdir;
use uuid::Uuid;

#[tokio::test]
async fn backup_restore_roundtrip() {
    let dir = tempdir().unwrap();
    let metrics = Arc::new(WalMetrics::default());
    let engine = StorageEngine::new(Arc::clone(&metrics));
    let cid = Uuid::now_v7();
    {
        let mut c = engine.content.write().await;
        c.register_mode(cid, StorageMode::Persistent);
        c.put(cid, b"k".to_vec(), b"v".to_vec());
    }

    let svc = JobService::new(true);
    let orch = BackupOrchestrator {
        store: svc.store.clone(),
        slice10: true,
    };
    let job = JobRecord::new(
        JobKind::DataBackup,
        Uuid::nil(),
        Strategy::Snapshot,
        serde_json::json!({}),
    );
    let meta = ContainerSnapshotMeta {
        id: cid,
        namespace: "acme".into(),
        name: "t".into(),
        type_name: "kv_store".into(),
        mode: "persistent".into(),
        key_ref: None,
    };
    let done = orch
        .run_backup(
            job,
            &BackupSpec {
                scope: BackupScope::Namespace {
                    name: "acme".into(),
                },
                pitr: true,
                containers: vec![meta],
                available_key_refs: vec![],
            },
            &engine,
            dir.path().to_path_buf(),
            &AuthzContext::cluster_admin(),
        )
        .await
        .unwrap();
    assert!(done.snapshot_id.is_some());
    assert!(done.body.get("positions").is_some());

    {
        let mut c = engine.content.write().await;
        c.put(cid, b"k".to_vec(), b"changed".to_vec());
    }

    let restore_job = JobRecord::new(
        JobKind::DataRestore,
        Uuid::nil(),
        Strategy::Snapshot,
        serde_json::json!({}),
    );
    orch.run_restore(
        restore_job,
        &RestoreSpec {
            snapshot_id: done.snapshot_id.unwrap(),
            confirm_drop: true,
            key_ref: None,
            dest_namespace: None,
            key_refs: vec![],
        },
        &engine,
        dir.path().to_path_buf(),
        &AuthzContext::cluster_admin(),
    )
    .await
    .unwrap();

    let c = engine.content.read().await;
    assert_eq!(c.get(cid, b"k"), Some(b"v".as_slice()));
}

#[tokio::test]
async fn encrypted_backup_missing_key() {
    let dir = tempdir().unwrap();
    let metrics = Arc::new(WalMetrics::default());
    let engine = StorageEngine::new(Arc::clone(&metrics));
    let cid = Uuid::now_v7();
    {
        let mut c = engine.content.write().await;
        c.register_mode(cid, StorageMode::Persistent);
        c.put(cid, b"k".to_vec(), b"v".to_vec());
    }
    let orch = BackupOrchestrator {
        store: JobService::new(true).store.clone(),
        slice10: true,
    };
    let job = JobRecord::new(
        JobKind::DataBackup,
        Uuid::nil(),
        Strategy::Snapshot,
        serde_json::json!({}),
    );
    let err = orch
        .run_backup(
            job,
            &BackupSpec {
                scope: BackupScope::Container {
                    namespace: "acme".into(),
                    container: "enc".into(),
                },
                pitr: false,
                containers: vec![ContainerSnapshotMeta {
                    id: cid,
                    namespace: "acme".into(),
                    name: "enc".into(),
                    type_name: "kv_store".into(),
                    mode: "persistent".into(),
                    key_ref: Some("key/missing".into()),
                }],
                available_key_refs: vec![],
            },
            &engine,
            dir.path().to_path_buf(),
            &AuthzContext::cluster_admin(),
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "KeyRefMissing");
}
