//! Fixture + dual-write + first-binary stub unit tests (010).

use spacestorage_migrate::{
    apply_mapped, incomplete_name, resolve_mapping, stub_service, AuthzContext, CutoverGates,
    DualWriteRegistry, DualWriteWindow, JobKind, JobService, MappingQuery, MappingSource,
    MigrateError, QuotaView, Strategy,
};
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

#[test]
fn first_binary_stub_refuses() {
    let svc = stub_service();
    let err = svc
        .start(
            JobKind::DataMigration,
            Uuid::nil(),
            Strategy::Live,
            serde_json::json!({
                "source": {"namespace":"a","container":"b"},
                "dest_nodes":["n2"]
            }),
            &AuthzContext::cluster_admin(),
        )
        .unwrap_err();
    assert_eq!(err.code(), "MigrateSlice10Required");
}

#[test]
fn evacuate_fixture_shape() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../specs/010-migration-transforms/contracts/fixtures/evacuate-live.json");
    let text = std::fs::read_to_string(&path).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["kind"], "data_migration");
    assert_eq!(v["strategy"], "live");
    let svc = JobService::new(true);
    let job = svc
        .start(
            JobKind::parse(v["kind"].as_str().unwrap()).unwrap(),
            Uuid::nil(),
            Strategy::parse(v["strategy"].as_str().unwrap()).unwrap(),
            v,
            &AuthzContext::cluster_admin(),
        )
        .unwrap();
    assert_eq!(job.kind, JobKind::DataMigration);
}

#[test]
fn dual_write_sequence_and_cutover() {
    let reg = DualWriteRegistry::new();
    let id = Uuid::now_v7();
    let job = Uuid::now_v7();
    reg.register(DualWriteWindow {
        container_id: id,
        job_id: job,
        install_seq: 5,
        target_id: Uuid::now_v7(),
    })
    .unwrap();
    assert!(matches!(
        reg.register(DualWriteWindow {
            container_id: id,
            job_id: Uuid::now_v7(),
            install_seq: 6,
            target_id: Uuid::now_v7(),
        }),
        Err(MigrateError::JobInProgress)
    ));

    let mut applied = HashMap::new();
    let mut dest = HashMap::new();
    assert!(apply_mapped(
        &mut applied,
        b"k".to_vec(),
        1,
        b"v".to_vec(),
        &mut dest
    ));
    assert!(!apply_mapped(
        &mut applied,
        b"k".to_vec(),
        1,
        b"v2".to_vec(),
        &mut dest
    ));
    assert_eq!(dest.get(b"k".as_slice()).map(|v| v.as_slice()), Some(b"v".as_slice()));

    let gates = CutoverGates {
        last_applied_source_seq: 5,
        source_head_seq: 5,
        quota: QuotaView::default(),
        additional_bytes: 0,
        placement_ok: true,
        target_complete: true,
        dest_name_free_or_swap: true,
    };
    gates.check().unwrap();
    assert!(incomplete_name(job).starts_with("ss:job:"));
}

#[test]
fn mapping_catalog_vs_query() {
    resolve_mapping("document_store", "relational_table", None, true).unwrap();
    let q = MappingQuery {
        sources: vec![MappingSource {
            namespace: "a".into(),
            container: "t".into(),
            columns: vec!["*".into()],
        }],
        destinations: vec![MappingSource {
            namespace: "a".into(),
            container: "t2".into(),
            columns: vec!["*".into()],
        }],
        filter: Some(serde_json::json!({"join": true})),
    };
    assert_eq!(
        resolve_mapping("a", "b", Some(&q), true)
            .unwrap_err()
            .code(),
        "NotSupported"
    );
}
