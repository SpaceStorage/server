//! Admin UI cluster map conformance (009 US1) — feature `complete-product`.

#![cfg(feature = "complete-product")]

use spacestorage_admin_ui::{
    ClusterMapView, ContainerMapEntry, MapAuthz, MapComposer, MapScope, MigrationProgress,
    NodeMapEntry, ReplicaMapEntry, ShardMapEntry, TopologySnapshot, UiPrincipal,
};

fn lag_topology() -> TopologySnapshot {
    TopologySnapshot {
        nodes: vec![
            NodeMapEntry {
                id: "n1".into(),
                name: "n1".into(),
                ready: true,
            },
            NodeMapEntry {
                id: "n2".into(),
                name: "n2".into(),
                ready: true,
            },
            NodeMapEntry {
                id: "n3".into(),
                name: "n3".into(),
                ready: true,
            },
        ],
        containers: vec![
            ContainerMapEntry {
                namespace: "acme".into(),
                name: "events".into(),
                type_name: "log_stream".into(),
                shards: vec![ShardMapEntry {
                    id: "s0".into(),
                    replicas: vec![
                        ReplicaMapEntry {
                            id: "r0".into(),
                            node: "n1".into(),
                            health: "ok".into(),
                            lag_ms: 0,
                        },
                        ReplicaMapEntry {
                            id: "r1".into(),
                            node: "n2".into(),
                            health: "degraded".into(),
                            lag_ms: 4200,
                        },
                    ],
                }],
            },
            ContainerMapEntry {
                namespace: "other".into(),
                name: "x".into(),
                type_name: "kv_store".into(),
                shards: vec![],
            },
        ],
        migrations: vec![MigrationProgress {
            id: "mig-1".into(),
            status: "running".into(),
            bytes_copied: 10,
            bytes_remaining: 90,
        }],
    }
}

#[test]
fn cluster_admin_sees_lag_matching_topology() {
    let c = MapComposer::with_topology(lag_topology());
    let view = c.compose(&MapAuthz::Cluster, None).unwrap();
    assert_eq!(view.scope, MapScope::Cluster);
    let lag = view
        .containers
        .iter()
        .flat_map(|c| c.shards.iter())
        .flat_map(|s| s.replicas.iter())
        .find(|r| r.lag_ms > 0)
        .expect("lagging replica");
    assert_eq!(lag.id, "r1");
    assert_eq!(lag.lag_ms, 4200);
}

#[test]
fn namespace_admin_scope_hides_other_tenants() {
    let c = MapComposer::with_topology(lag_topology());
    let view = c
        .compose(&MapAuthz::Namespace(vec!["acme".into()]), None)
        .unwrap();
    assert_eq!(view.scope, MapScope::Namespace);
    assert!(view.containers.iter().all(|c| c.namespace == "acme"));
    assert!(!view.containers.iter().any(|c| c.namespace == "other"));
}

#[test]
fn unprivileged_forbidden() {
    let c = MapComposer::with_topology(lag_topology());
    assert!(c.compose(&UiPrincipal::unprivileged("x").map_authz(), None).is_err());
}

#[test]
fn migrations_expose_progress_fields() {
    let c = MapComposer::with_topology(lag_topology());
    let view: ClusterMapView = c.compose(&MapAuthz::Cluster, None).unwrap();
    let m = &view.migrations[0];
    assert_eq!(m.id, "mig-1");
    assert_eq!(m.status, "running");
    assert_eq!(m.bytes_copied, 10);
    assert_eq!(m.bytes_remaining, 90);
}
