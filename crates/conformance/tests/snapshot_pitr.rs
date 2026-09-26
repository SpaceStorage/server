//! Snapshot / PITR conformance SC-006–SC-009 (013 US4).

#![cfg(feature = "migration-backup")]

use spacestorage_backup::{
    map_hlc_to_positions, scope_container, ContainerSnapshotMeta, PitrTarget, RestoreRequest,
    RestoreService, SnapshotCreateRequest, SnapshotService, StampLsn,
};
use spacestorage_storage::{StorageEngine, StorageMode, WalMetrics};
use std::collections::BTreeMap;
use std::sync::Arc;
use tempfile::tempdir;
use uuid::Uuid;

#[tokio::test]
async fn snapshot_positions_and_pitr_map() {
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
    assert!(!man.positions.is_empty() || man.positions.get("default") == Some(&0));

    let mut stamps = BTreeMap::new();
    stamps.insert(
        "default".into(),
        vec![
            StampLsn { stamp: 10, lsn: 1 },
            StampLsn { stamp: 20, lsn: 2 },
        ],
    );
    let mut durable = BTreeMap::new();
    durable.insert("default".into(), 10u64);
    let pos = map_hlc_to_positions(15, &stamps, &durable).unwrap();
    assert_eq!(pos.get("default"), Some(&1));

    let unmap = map_hlc_to_positions(5, &stamps, &durable).unwrap_err();
    assert!(unmap.to_string().contains("d") || unmap.to_string().contains("pitr"));

    let restore = RestoreService::new(dir.path().to_path_buf(), true);
    {
        let mut c = engine.content.write().await;
        c.put(cid, b"k".to_vec(), b"x".to_vec());
    }
    let err = restore
        .restore_fill(
            &engine,
            RestoreRequest {
                snapshot_id: man.snapshot_id,
                confirm_drop: false,
                pitr: Some(PitrTarget::Positions {
                    positions: man.positions.clone(),
                }),
                key_refs: vec![],
                wal_stamps: stamps,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "RestoreDestinationHasContent");
}

#[tokio::test]
async fn first_binary_snapshot_service_refuses() {
    let dir = tempdir().unwrap();
    let metrics = Arc::new(WalMetrics::default());
    let engine = StorageEngine::new(Arc::clone(&metrics));
    let svc = SnapshotService::new(dir.path().to_path_buf(), false);
    let err = svc
        .create(
            &engine,
            SnapshotCreateRequest {
                data_dir: dir.path().to_path_buf(),
                scope: scope_container("a", "b"),
                containers: vec![],
                available_key_refs: vec![],
            },
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "BackupSlice10Required");
}
