//! Cluster map view composer (004/006/010/011 filter, not private topology).

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MapScope {
    Cluster,
    Namespace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapAuthz {
    Cluster,
    Namespace(Vec<String>),
    Forbidden,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClusterMapView {
    pub generated_at: String,
    pub scope: MapScope,
    pub nodes: Vec<NodeMapEntry>,
    pub containers: Vec<ContainerMapEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub migrations: Vec<MigrationProgress>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeMapEntry {
    pub id: String,
    pub name: String,
    pub ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContainerMapEntry {
    pub namespace: String,
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
    pub shards: Vec<ShardMapEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShardMapEntry {
    pub id: String,
    pub replicas: Vec<ReplicaMapEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplicaMapEntry {
    pub id: String,
    pub node: String,
    pub health: String,
    pub lag_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MigrationProgress {
    pub id: String,
    pub status: String,
    pub bytes_copied: u64,
    pub bytes_remaining: u64,
}

#[derive(Debug, Error)]
pub enum MapError {
    #[error("forbidden")]
    Forbidden,
}

/// In-memory topology snapshot used by the map composer (tests + node inject).
#[derive(Debug, Clone, Default)]
pub struct TopologySnapshot {
    pub nodes: Vec<NodeMapEntry>,
    pub containers: Vec<ContainerMapEntry>,
    pub migrations: Vec<MigrationProgress>,
}

#[derive(Debug, Clone, Default)]
pub struct MapComposer {
    pub topology: TopologySnapshot,
}

impl MapComposer {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn with_topology(topology: TopologySnapshot) -> Self {
        Self { topology }
    }

    pub fn compose(
        &self,
        authz: &MapAuthz,
        namespace_query: Option<&str>,
    ) -> Result<ClusterMapView, MapError> {
        match authz {
            MapAuthz::Forbidden => Err(MapError::Forbidden),
            MapAuthz::Cluster => {
                let mut view = self.full_view(MapScope::Cluster);
                if let Some(ns) = namespace_query {
                    view = filter_namespace(view, ns);
                    view.scope = MapScope::Namespace;
                }
                Ok(view)
            }
            MapAuthz::Namespace(allowed) => {
                let ns = namespace_query
                    .map(|s| s.to_string())
                    .or_else(|| allowed.first().cloned())
                    .ok_or(MapError::Forbidden)?;
                if !allowed.iter().any(|a| a == &ns) {
                    return Err(MapError::Forbidden);
                }
                Ok(filter_namespace(self.full_view(MapScope::Namespace), &ns))
            }
        }
    }

    fn full_view(&self, scope: MapScope) -> ClusterMapView {
        ClusterMapView {
            generated_at: chrono_now(),
            scope,
            nodes: self.topology.nodes.clone(),
            containers: self.topology.containers.clone(),
            migrations: self.topology.migrations.clone(),
        }
    }
}

fn filter_namespace(mut view: ClusterMapView, ns: &str) -> ClusterMapView {
    view.containers.retain(|c| c.namespace == ns);
    let host_nodes: std::collections::BTreeSet<String> = view
        .containers
        .iter()
        .flat_map(|c| c.shards.iter())
        .flat_map(|s| s.replicas.iter())
        .map(|r| r.node.clone())
        .collect();
    view.nodes.retain(|n| host_nodes.contains(&n.name) || host_nodes.contains(&n.id));
    view.migrations.retain(|_m| true); // migrations already scoped by caller data
    view.scope = MapScope::Namespace;
    view
}

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{ms}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_topology() -> TopologySnapshot {
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
                                lag_ms: 5000,
                            },
                        ],
                    }],
                },
                ContainerMapEntry {
                    namespace: "other".into(),
                    name: "data".into(),
                    type_name: "kv_store".into(),
                    shards: vec![ShardMapEntry {
                        id: "s1".into(),
                        replicas: vec![ReplicaMapEntry {
                            id: "r2".into(),
                            node: "n3".into(),
                            health: "ok".into(),
                            lag_ms: 0,
                        }],
                    }],
                },
            ],
            migrations: vec![MigrationProgress {
                id: "m1".into(),
                status: "running".into(),
                bytes_copied: 100,
                bytes_remaining: 50,
            }],
        }
    }

    #[test]
    fn namespace_scope_hides_foreign_containers() {
        let c = MapComposer::with_topology(sample_topology());
        let view = c
            .compose(&MapAuthz::Namespace(vec!["acme".into()]), None)
            .unwrap();
        assert_eq!(view.scope, MapScope::Namespace);
        assert!(view.containers.iter().all(|c| c.namespace == "acme"));
        assert_eq!(view.containers.len(), 1);
        // n3 only hosts other — must not appear
        assert!(!view.nodes.iter().any(|n| n.name == "n3"));
        assert!(view.nodes.iter().any(|n| n.name == "n1"));
    }

    #[test]
    fn forbidden_returns_no_body_error() {
        let c = MapComposer::with_topology(sample_topology());
        assert!(matches!(
            c.compose(&MapAuthz::Forbidden, None),
            Err(MapError::Forbidden)
        ));
    }

    #[test]
    fn cluster_sees_lagging_replica() {
        let c = MapComposer::with_topology(sample_topology());
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
        assert_eq!(view.migrations[0].bytes_remaining, 50);
    }
}
