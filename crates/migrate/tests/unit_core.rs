//! Fixture + dual-write + first-binary stub unit tests (010).

use spacestorage_migrate::{
    AuthzContext, CutoverGates, DualWriteRegistry, DualWriteWindow, JobKind, JobService,
    MappingQuery, MappingSource, MigrateError, QuotaView, Strategy, apply_mapped, incomplete_name,
    resolve_mapping, stub_service,
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
        rewrite: None,
    })
    .unwrap();
    assert!(matches!(
        reg.register(DualWriteWindow {
            container_id: id,
            job_id: Uuid::now_v7(),
            install_seq: 6,
            target_id: Uuid::now_v7(),
            rewrite: None,
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
    assert_eq!(
        dest.get(b"k".as_slice()).map(|v| v.as_slice()),
        Some(b"v".as_slice())
    );

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

#[test]
fn transform_rewrite_changes_bytes_not_identity() {
    use spacestorage_migrate::{MappingSpec, apply_row_transform, catalog_default};
    let mapping = MappingSpec::Catalog(catalog_default("kv_store", "kv_store").unwrap());
    let (k, v) = apply_row_transform("re_encode", &mapping, b"k", b"v");
    assert_eq!(k, b"k");
    assert_ne!(v, b"v");
    assert!(v.starts_with(b"re_encode/v1|"));
    let (sk, sv) = apply_row_transform("sharding_key", &mapping, b"k", b"v");
    assert_eq!(sk, b"sk:k");
    assert_eq!(sv, b"v");
}

#[test]
fn dual_write_intercept_buffers_target() {
    let reg = DualWriteRegistry::new();
    let src = Uuid::now_v7();
    let tgt = Uuid::now_v7();
    let job = Uuid::now_v7();
    reg.register(DualWriteWindow {
        container_id: src,
        job_id: job,
        install_seq: 1,
        target_id: tgt,
        rewrite: None,
    })
    .unwrap();
    let seq = reg
        .intercept_source_write(src, b"k", b"v", |_tid, _k, _v| Ok(()), |_jid, _e| {})
        .unwrap();
    assert!(seq.is_some());
    let pending = reg.pending_for(tgt);
    assert_eq!(
        pending.get(b"k".as_slice()).map(|v| v.as_slice()),
        Some(b"v".as_slice())
    );
    assert_eq!(reg.grown_source_bytes(src, 10), 12); // baseline 10 + len(k)+len(v)
}

#[test]
fn dual_write_target_fail_pauses_and_does_not_ack() {
    let reg = DualWriteRegistry::new();
    let src = Uuid::now_v7();
    let tgt = Uuid::now_v7();
    let job = Uuid::now_v7();
    reg.register(DualWriteWindow {
        container_id: src,
        job_id: job,
        install_seq: 1,
        target_id: tgt,
        rewrite: None,
    })
    .unwrap();
    let mut paused_job = None;
    let err = reg
        .intercept_source_write(
            src,
            b"k",
            b"v",
            |_tid, _k, _v| Err(MigrateError::Io("target quorum failed".into())),
            |jid, e| {
                paused_job = Some((jid, e.code().to_string()));
            },
        )
        .unwrap_err();
    assert_eq!(err.code(), "io");
    assert_eq!(paused_job, Some((job, "io".into())));
    assert!(reg.pending_for(tgt).is_empty());
}

#[test]
fn dual_write_rewrite_maps_interceptor_puts() {
    use spacestorage_migrate::{DualWriteRewrite, MappingSpec, catalog_default};
    let reg = DualWriteRegistry::new();
    let src = Uuid::now_v7();
    let tgt = Uuid::now_v7();
    let job = Uuid::now_v7();
    let mapping = MappingSpec::Catalog(catalog_default("kv_store", "kv_store").unwrap());
    reg.register(DualWriteWindow {
        container_id: src,
        job_id: job,
        install_seq: 1,
        target_id: tgt,
        rewrite: Some(DualWriteRewrite {
            kind: "re_encode".into(),
            mapping,
        }),
    })
    .unwrap();
    let mut applied = None;
    reg.intercept_source_write(
        src,
        b"k",
        b"v",
        |_tid, k, v| {
            applied = Some((k.to_vec(), v.to_vec()));
            Ok(())
        },
        |_jid, _e| {},
    )
    .unwrap();
    let (ak, av) = applied.expect("mapped apply");
    assert_eq!(ak, b"k");
    assert!(av.starts_with(b"re_encode/v1|"));
    let pending = reg.pending_for(tgt);
    assert_eq!(
        pending.get(b"k".as_slice()).map(|v| v.as_slice()),
        Some(av.as_slice())
    );
}

#[test]
fn cutover_quota_uses_grown_source_bytes() {
    let view = QuotaView {
        logical_usage: 8,
        soft_limit: Some(12),
    };
    // Baseline 3 fits; grown 5 (as if dual-write added 2) does not.
    view.check_fits(3).unwrap();
    assert_eq!(view.check_fits(5).unwrap_err().code(), "QuotaExceeded");
    let gates = CutoverGates {
        last_applied_source_seq: 1,
        source_head_seq: 1,
        quota: view,
        additional_bytes: 5,
        placement_ok: true,
        target_complete: true,
        dest_name_free_or_swap: true,
    };
    assert_eq!(gates.check().unwrap_err().code(), "QuotaExceeded");
}

#[test]
fn pending_drain_includes_puts_after_first_take() {
    let reg = DualWriteRegistry::new();
    let src = Uuid::now_v7();
    let tgt = Uuid::now_v7();
    reg.register(DualWriteWindow {
        container_id: src,
        job_id: Uuid::now_v7(),
        install_seq: 1,
        target_id: tgt,
        rewrite: None,
    })
    .unwrap();
    reg.buffer_target_put(tgt, b"k1".to_vec(), b"v1".to_vec());
    let mut got = HashMap::new();
    let mut reinjected = false;
    loop {
        let pending = reg.take_pending(tgt);
        if pending.is_empty() {
            break;
        }
        if !reinjected {
            // Put acked after the first drain — must be picked up on the next iteration.
            reg.buffer_target_put(tgt, b"k2".to_vec(), b"v2".to_vec());
            reinjected = true;
        }
        got.extend(pending);
    }
    let leftover = reg.unregister(src);
    got.extend(leftover);
    assert_eq!(
        got.get(b"k1".as_slice()).map(|v| v.as_slice()),
        Some(b"v1".as_slice())
    );
    assert_eq!(
        got.get(b"k2".as_slice()).map(|v| v.as_slice()),
        Some(b"v2".as_slice())
    );
}

#[test]
fn transform_swap_keeps_source_rows_and_publishes_target() {
    use spacestorage_migrate::{ContainerRef, JobStatus, RewriteKind, Strategy, TransformSpec};
    use spacestorage_storage::{ContentStore, StorageMode};

    let svc = JobService::new(true);
    let runner = svc.transform_runner();
    let source_id = Uuid::now_v7();
    let mut rows = HashMap::new();
    rows.insert(b"a".to_vec(), b"1".to_vec());
    let mut names = HashMap::new();
    names.insert(("acme".into(), "t".into()), source_id);
    let mut content = ContentStore::default();
    content.register_mode(source_id, StorageMode::Persistent);
    content.put(source_id, b"a".to_vec(), b"1".to_vec());

    let spec = TransformSpec {
        source: ContainerRef {
            namespace: "acme".into(),
            container: "t".into(),
        },
        rewrite: RewriteKind::ReEncode,
        target_type: Some("kv_store".into()),
        mapping_query: None,
        new_name: Some("t_new".into()),
        swap: true,
        retain_source: true,
        source_type: "kv_store".into(),
    };
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

    // Seed a mapped dual-write that arrives during the job window, then ensure run drains it.
    // Register happens inside run; buffer via the shared dual after a synchronous intercept
    // once the window exists — use the service dual + a pre-run register is rejected, so
    // inject by buffering on target after start using a parallel register is impossible.
    // Cover drain via pending_drain_includes_puts_after_first_take; here assert swap publish.
    let done = runner
        .run(
            job,
            &spec,
            source_id,
            &rows,
            1,
            &mut names,
            &mut content,
            QuotaView::default(),
            2,
        )
        .unwrap();
    assert_eq!(done.status, JobStatus::Completed);
    let swapped = names.get(&("acme".into(), "t".into())).copied();
    assert!(swapped.is_some());
    assert_ne!(swapped, Some(source_id));
    // Source content left intact when retain_source (not wiped by stale snapshot publish).
    assert_eq!(
        content.get(source_id, b"a").as_deref(),
        Some(b"1".as_slice())
    );
    let tid = swapped.unwrap();
    let exported = content.export_durable(tid);
    assert!(exported.values().any(|v| v.starts_with(b"re_encode/v1|")));
    // T078: dual-write window stays registered until after catalog publish.
    assert!(svc.dual.get(source_id).is_some());
    let _ = svc.dual.unregister(source_id);
    assert!(svc.dual.get(source_id).is_none());
}

#[test]
fn plan_replicas_refuses_unsatisfiable_anti_affinity() {
    use spacestorage_migrate::{ContainerRef, MigratePolicy, MigrationSpec, PlacementCheck};
    use spacestorage_placement::topology::NodeTopo;
    use std::collections::BTreeMap;

    let spec = MigrationSpec {
        source: ContainerRef {
            namespace: "acme".into(),
            container: "kv".into(),
        },
        dest_namespace: None,
        dest_name: Some("kv_b".into()),
        policy: MigratePolicy::Copy,
        dest_nodes: Some(vec!["n1".into(), "n2".into(), "n3".into()]),
        dest_drives: None,
        strategy: Strategy::Live,
        force_unsatisfiable: false,
    };
    // Three nodes, same az — RF=3 with default az anti-affinity is unsatisfiable.
    let check = PlacementCheck {
        candidates: vec![
            NodeTopo {
                name: "n1".into(),
                labels: BTreeMap::from([("az".into(), "a".into())]),
                quorum_domain: "default".into(),
                no_tenant_data: false,
            },
            NodeTopo {
                name: "n2".into(),
                labels: BTreeMap::from([("az".into(), "a".into())]),
                quorum_domain: "default".into(),
                no_tenant_data: false,
            },
            NodeTopo {
                name: "n3".into(),
                labels: BTreeMap::from([("az".into(), "a".into())]),
                quorum_domain: "default".into(),
                no_tenant_data: false,
            },
        ],
        topology_ladder: vec!["az".into()],
        replication_factor: 3,
        anti_affinity_keys: vec![],
        available_drives: vec![],
    };
    let err = spec.validate_destination_placement(&check).unwrap_err();
    assert_eq!(err.code(), "ConstraintUnsatisfiable");
}

#[test]
fn mapping_query_executes_via_005_logical_request() {
    use spacestorage_migrate::{
        MappingQuery, MappingSource, execute_mapping_query, lower_to_logical_request,
    };
    use spacestorage_query::LogicalRequest;

    let q = MappingQuery {
        sources: vec![MappingSource {
            namespace: "acme".into(),
            container: "events".into(),
            columns: vec!["*".into()],
        }],
        destinations: vec![MappingSource {
            namespace: "acme".into(),
            container: "events_out".into(),
            columns: vec!["*".into()],
        }],
        filter: None,
    };
    assert!(matches!(
        lower_to_logical_request(&q),
        LogicalRequest::Batch { .. }
    ));
    let mut rows = HashMap::new();
    rows.insert(b"k1".to_vec(), b"v1".to_vec());
    let out = execute_mapping_query(&q, &rows).unwrap();
    assert_eq!(
        out.get(b"k1".as_slice()).map(|v| v.as_slice()),
        Some(b"v1".as_slice())
    );
}

#[test]
fn quota_admit_refuses_second_copy() {
    use spacestorage_migrate::admit_dest_bytes;
    use spacestorage_tenancy::Tenancy;

    let view = QuotaView {
        logical_usage: 8,
        soft_limit: Some(10),
    };
    assert_eq!(view.check_fits(5).unwrap_err().code(), "QuotaExceeded");
    // Unlimited namespace (no `07` bytes quota) admits.
    let tenancy = Tenancy::new();
    admit_dest_bytes(&tenancy, "acme", 1_000).unwrap();
}

#[test]
fn start_fails_state_unreadable_on_persist_error() {
    use std::sync::Arc;
    let svc = JobService::new(true);
    svc.set_persist_hook(Some(Arc::new(|_job| Err("append boom".into()))));
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
    assert_eq!(err.code(), "StateUnreadable");
}

#[test]
fn restore_spec_pitr_position_maps_to_target() {
    use spacestorage_backup::PitrTarget;
    use spacestorage_migrate::PitrPosition;
    use std::collections::BTreeMap;

    let pos = PitrPosition::Positions(BTreeMap::from([("d1".into(), 42u64)]));
    assert!(matches!(
        pos.into_pitr_target(),
        PitrTarget::Positions { positions } if positions.get("d1") == Some(&42)
    ));
}
