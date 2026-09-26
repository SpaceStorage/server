//! Five rewrite kinds + mapping refuse (010 SC-003).

#![cfg(feature = "migration-backup")]

use spacestorage_migrate::{
    resolve_mapping, AuthzContext, ContainerRef, JobKind, JobService, RewriteKind, Strategy,
    TransformSpec,
};
use spacestorage_storage::{ContentStore, StorageMode};
use std::collections::HashMap;
use uuid::Uuid;

#[test]
fn five_rewrite_kinds_run() {
    let svc = JobService::new(true);
    let runner = svc.transform_runner();
    let kinds = [
        RewriteKind::TypeModel,
        RewriteKind::IncompatibleSchema,
        RewriteKind::ReEncode,
        RewriteKind::ReCompress,
        RewriteKind::ReEncrypt,
        RewriteKind::ShardingKey,
    ];
    for (i, rewrite) in kinds.into_iter().enumerate() {
        let source_id = Uuid::now_v7();
        let mut rows = HashMap::new();
        rows.insert(b"id".to_vec(), b"1".to_vec());
        let mut names = HashMap::new();
        let cname = format!("c{i}");
        names.insert(("acme".into(), cname.clone()), source_id);
        let mut content = ContentStore::default();
        content.register_mode(source_id, StorageMode::Persistent);
        content.put(source_id, b"id".to_vec(), b"1".to_vec());

        let mut spec = TransformSpec {
            source: ContainerRef {
                namespace: "acme".into(),
                container: cname.clone(),
            },
            rewrite,
            target_type: Some("relational_table".into()),
            mapping_query: None,
            new_name: Some(format!("{cname}_new")),
            swap: false,
            retain_source: true,
            source_type: "document_store".into(),
        };
        if rewrite.identity_ok() {
            spec.source_type = "kv_store".into();
            spec.target_type = Some("kv_store".into());
        }
        let body = serde_json::to_value(&spec).unwrap();
        let job = svc
            .start(
                JobKind::DataTransformation,
                Uuid::nil(),
                Strategy::Live,
                body,
                &AuthzContext::cluster_admin(),
            )
            .unwrap();
        let done = runner
            .run(job, &spec, source_id, &rows, 1, &mut names, &mut content)
            .unwrap();
        assert_eq!(done.status, spacestorage_migrate::JobStatus::Completed);
    }
}

#[test]
fn mapping_query_required_for_complex() {
    let err = resolve_mapping("weird_a", "weird_b", None, true).unwrap_err();
    assert_eq!(err.code(), "MappingQueryRequired");
}
