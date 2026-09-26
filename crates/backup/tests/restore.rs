//! Restore confirm_drop unit tests (013 T043).

use spacestorage_backup::{
    scope_container, ContainerSnapshotMeta, RestoreRequest, RestoreService, SnapshotCreateRequest,
    SnapshotService,
};
use spacestorage_storage::{StorageEngine, StorageMode, WalMetrics};
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::tempdir;
use uuid::Uuid;

#[tokio::test]
async fn confirm_drop_false_refuses_when_dest_has_content() {
    let dir = tempdir().unwrap();
    let metrics = Arc::new(WalMetrics::default());
    let engine = StorageEngine::new(Arc::clone(&metrics));
    let cid = Uuid::now_v7();
    {
        let mut c = engine.content.write().await;
        c.register_mode(cid, StorageMode::Persistent);
        c.put(cid, b"k".to_vec(), b"v".to_vec());
    }

    let svc = SnapshotService::new(dir.path().to_path_buf(), true);
    let man = svc
        .create(
            &engine,
            SnapshotCreateRequest {
                data_dir: dir.path().to_path_buf(),
                scope: scope_container("acme", "t"),
                containers: vec![ContainerSnapshotMeta {
                    id: cid,
                    namespace: "acme".into(),
                    name: "t".into(),
                    type_name: "kv_store".into(),
                    mode: "persistent".into(),
                    key_ref: None,
                }],
                available_key_refs: vec![],
            },
        )
        .await
        .unwrap();

    // Put different content so dest is non-empty.
    {
        let mut c = engine.content.write().await;
        c.put(cid, b"k2".to_vec(), b"v2".to_vec());
    }

    let restore = RestoreService::new(dir.path().to_path_buf(), true);
    let err = restore
        .restore_fill(
            &engine,
            RestoreRequest {
                snapshot_id: man.snapshot_id,
                confirm_drop: false,
                pitr: None,
                key_refs: vec![],
                wal_stamps: Default::default(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "RestoreDestinationHasContent");
    // Dest intact
    let c = engine.content.read().await;
    assert_eq!(c.get(cid, b"k2"), Some(b"v2".as_slice()));
}

#[tokio::test]
async fn confirm_drop_true_fills() {
    let dir = tempdir().unwrap();
    let metrics = Arc::new(WalMetrics::default());
    let engine = StorageEngine::new(Arc::clone(&metrics));
    let cid = Uuid::now_v7();
    {
        let mut c = engine.content.write().await;
        c.register_mode(cid, StorageMode::Persistent);
        c.put(cid, b"k".to_vec(), b"orig".to_vec());
    }

    let svc = SnapshotService::new(dir.path().to_path_buf(), true);
    let man = svc
        .create(
            &engine,
            SnapshotCreateRequest {
                data_dir: dir.path().to_path_buf(),
                scope: scope_container("acme", "t"),
                containers: vec![ContainerSnapshotMeta {
                    id: cid,
                    namespace: "acme".into(),
                    name: "t".into(),
                    type_name: "kv_store".into(),
                    mode: "persistent".into(),
                    key_ref: None,
                }],
                available_key_refs: vec![],
            },
        )
        .await
        .unwrap();

    {
        let mut c = engine.content.write().await;
        c.put(cid, b"k".to_vec(), b"changed".to_vec());
    }

    let restore = RestoreService::new(dir.path().to_path_buf(), true);
    restore
        .restore_fill(
            &engine,
            RestoreRequest {
                snapshot_id: man.snapshot_id,
                confirm_drop: true,
                pitr: None,
                key_refs: vec![],
                wal_stamps: Default::default(),
            },
        )
        .await
        .unwrap();

    let c = engine.content.read().await;
    assert_eq!(c.get(cid, b"k"), Some(b"orig".as_slice()));
}

#[tokio::test]
async fn memory_mode_omitted_from_snapshot() {
    let dir = tempdir().unwrap();
    let metrics = Arc::new(WalMetrics::default());
    let engine = StorageEngine::new(Arc::clone(&metrics));
    let cid = Uuid::now_v7();
    {
        let mut c = engine.content.write().await;
        c.register_mode(cid, StorageMode::Memory);
        // put is a no-op for memory; simulate via replace then clear on export
        let mut rows = HashMap::new();
        rows.insert(b"k".to_vec(), b"secret".to_vec());
        // Force into rows map then re-register as memory so export is empty.
        c.replace_durable(cid, StorageMode::Persistent, rows);
        c.register_mode(cid, StorageMode::Memory);
    }

    let svc = SnapshotService::new(dir.path().to_path_buf(), true);
    let man = svc
        .create(
            &engine,
            SnapshotCreateRequest {
                data_dir: dir.path().to_path_buf(),
                scope: scope_container("acme", "mem"),
                containers: vec![ContainerSnapshotMeta {
                    id: cid,
                    namespace: "acme".into(),
                    name: "mem".into(),
                    type_name: "kv_store".into(),
                    mode: "memory".into(),
                    key_ref: None,
                }],
                available_key_refs: vec![],
            },
        )
        .await
        .unwrap();

    {
        let mut c = engine.content.write().await;
        c.clear_content(cid);
        c.register_mode(cid, StorageMode::Memory);
    }

    let restore = RestoreService::new(dir.path().to_path_buf(), true);
    restore
        .restore_fill(
            &engine,
            RestoreRequest {
                snapshot_id: man.snapshot_id,
                confirm_drop: false,
                pitr: None,
                key_refs: vec![],
                wal_stamps: Default::default(),
            },
        )
        .await
        .unwrap();

    let c = engine.content.read().await;
    assert_eq!(c.len(cid), 0);
}

#[tokio::test]
async fn missing_key_ref_named() {
    let dir = tempdir().unwrap();
    let metrics = Arc::new(WalMetrics::default());
    let engine = StorageEngine::new(Arc::clone(&metrics));
    let cid = Uuid::now_v7();
    {
        let mut c = engine.content.write().await;
        c.register_mode(cid, StorageMode::Persistent);
        c.put(cid, b"k".to_vec(), b"v".to_vec());
    }

    let svc = SnapshotService::new(dir.path().to_path_buf(), true);
    let err = svc
        .create(
            &engine,
            SnapshotCreateRequest {
                data_dir: dir.path().to_path_buf(),
                scope: scope_container("acme", "enc"),
                containers: vec![ContainerSnapshotMeta {
                    id: cid,
                    namespace: "acme".into(),
                    name: "enc".into(),
                    type_name: "kv_store".into(),
                    mode: "persistent".into(),
                    key_ref: Some("key/prod".into()),
                }],
                available_key_refs: vec![],
            },
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "KeyRefMissing");
    assert!(err.to_string().contains("key/prod"));
}
