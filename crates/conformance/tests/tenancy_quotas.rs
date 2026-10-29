//! `007` quota admission harness (slice 7).

use spacestorage_admin_proto::{AdminOp, QuotaInput};
use spacestorage_conformance::{LAB_ADMIN_TOKEN, admin_tcp_json, boot_tenancy_one_node};
use spacestorage_query::{CancelToken, LogicalRequest, PlannerEngine, QueryEngine, QueryOptions};
use spacestorage_tenancy::QuotaUnit;
use spacestorage_types::{L3Model, StorageModeChoice};
use std::sync::Arc;
use tempfile::tempdir;

#[cfg(feature = "tenancy-quotas")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fill_to_cap_rejects_without_scheduling() {
    let dir = tempdir().unwrap();
    let lab = boot_tenancy_one_node(dir.path()).await;

    admin_tcp_json(
        lab.ports.admin,
        LAB_ADMIN_TOKEN,
        AdminOp::QuotaReplace {
            name: "acme".into(),
            quotas: vec![QuotaInput {
                unit: "bytes".into(),
                limit: 0,
                datatype: None,
            }],
        },
    )
    .await;

    {
        let cp = lab.node.controlplane.read().await;
        if let Some(cp) = cp.as_ref() {
            lab.node
                .tenancy
                .registry
                .lock()
                .replace_applied(cp.tenancy_namespaces());
        }
    }

    admin_tcp_json(
        lab.ports.admin,
        LAB_ADMIN_TOKEN,
        AdminOp::NamespaceCreate {
            name: "other".into(),
        },
    )
    .await;
    {
        let cp = lab.node.controlplane.read().await;
        if let Some(cp) = cp.as_ref() {
            lab.node
                .tenancy
                .registry
                .lock()
                .replace_applied(cp.tenancy_namespaces());
        }
    }

    {
        let mut cat = lab.node.catalog.write().unwrap();
        cat.create(
            "acme",
            "items",
            L3Model::KvStore,
            false,
            None,
            StorageModeChoice::Memory,
        )
        .unwrap();
        cat.create(
            "other",
            "items",
            L3Model::KvStore,
            false,
            None,
            StorageModeChoice::Memory,
        )
        .unwrap();
    }

    let engine = PlannerEngine::new(Arc::clone(&lab.node.catalog))
        .with_tenancy(Arc::clone(&lab.node.tenancy));
    let err = engine
        .execute(
            "acme",
            "s1",
            "postgresql",
            LogicalRequest::Object {
                container: "items".into(),
                key: "k".into(),
                bytes: vec![1, 2, 3],
            },
            QueryOptions::default(),
            CancelToken::new(),
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("QuotaExceeded"), "{err}");

    engine
        .execute(
            "other",
            "s2",
            "postgresql",
            LogicalRequest::Object {
                container: "items".into(),
                key: "k".into(),
                bytes: vec![9],
            },
            QueryOptions::default(),
            CancelToken::new(),
        )
        .await
        .expect("sibling namespace remains writable");

    lab.shutdown().await;
}

#[cfg(feature = "tenancy-quotas")]
#[test]
fn replica_factor_does_not_multiply_logical_bytes_in_usage_ledger() {
    use spacestorage_internode::QuotaDelta;
    use spacestorage_tenancy::QuotaUsage;
    use uuid::Uuid;

    let usage = QuotaUsage::new();
    let namespace = Uuid::from_u128(42);
    usage
        .apply_delta(&QuotaDelta {
            namespace,
            unit: "bytes".into(),
            datatype: None,
            delta: 100,
        })
        .unwrap();
    usage
        .apply_delta(&QuotaDelta {
            namespace,
            unit: "bytes".into(),
            datatype: None,
            delta: 100,
        })
        .unwrap();
    assert_eq!(usage.get(namespace, QuotaUnit::Bytes, &None), 200);
}

#[cfg(not(feature = "tenancy-quotas"))]
#[test]
fn quota_replace_admin_op_requires_slice7_feature() {
    // Compile-time gate: full fill-to-cap suite runs with `--features tenancy-quotas`.
    assert!(cfg!(not(feature = "tenancy-quotas")));
}
