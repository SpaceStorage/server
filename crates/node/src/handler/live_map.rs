//! Live cluster map snapshot from 011 membership, catalog, 010 jobs (009 T051).
//! Replica ids are never minted here — only reused when placement already exposes them.
//! Shard rows come from `004` describe (T098); empty when placement holds no replicas.

use crate::Node;
use spacestorage_admin_ui::{
    ContainerMapEntry, MigrationProgress, NodeMapEntry, ReplicaMapEntry, ShardMapEntry,
    TopologySnapshot,
};
use spacestorage_membership::events::MemberStatus;
use spacestorage_migrate::JobKind;
use spacestorage_placement::{DescribeSnapshot, PlacementEvent, apply_placement_event};
use std::sync::Arc;

/// Resolve the shared describe snapshot: Raft-applied state when controlplane is selected,
/// otherwise the node-local snapshot (single-node / pre-Raft).
pub async fn placement_describe_snapshot(node: &Arc<Node>) -> DescribeSnapshot {
    if let Some(cp) = node.controlplane.read().await.as_ref() {
        return cp.cluster_snapshot().replica_describe;
    }
    node.placement_describe
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Append `PlacementEvent::ReplicaHealth` through the node's `RaftClusterStore` when
/// selected; otherwise apply locally so describe still works without internodes (T099).
pub async fn append_placement_event(node: &Arc<Node>, event: PlacementEvent) -> Result<(), String> {
    if let Some(cp) = node.controlplane.read().await.as_ref() {
        cp.append_placement_event(event)
            .await
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    let mut snap = node
        .placement_describe
        .write()
        .unwrap_or_else(|e| e.into_inner());
    apply_placement_event(&mut snap, &event);
    Ok(())
}

pub async fn build_topology_snapshot(node: &Arc<Node>) -> TopologySnapshot {
    let mut nodes = Vec::new();
    if let Some(m) = node.membership.read().await.as_ref() {
        if let Some(view) = m.view_snapshot() {
            for (id, rec) in view.members {
                nodes.push(NodeMapEntry {
                    id: id.to_string(),
                    name: rec.node_name.clone(),
                    ready: matches!(rec.status, MemberStatus::Ready),
                });
            }
        }
    }
    if nodes.is_empty() {
        let cfg = node.config.load();
        nodes.push(NodeMapEntry {
            id: cfg.node_name.clone(),
            name: cfg.node_name.clone(),
            ready: matches!(node.state.get(), crate::lifecycle::NodeState::Ready),
        });
    }

    let describe = placement_describe_snapshot(node).await;

    let containers = {
        let cat = node.catalog.read().unwrap();
        cat.list()
            .map(|c| {
                let ns = c.namespace.as_str();
                let name = c.name.as_str();
                let rows = describe.describe_container(ns, name);
                let shards = if rows.is_empty() {
                    // No placement rows yet — do not invent shard/replica ids.
                    Vec::<ShardMapEntry>::new()
                } else {
                    vec![ShardMapEntry {
                        id: String::new(),
                        replicas: rows
                            .into_iter()
                            .map(|r| ReplicaMapEntry {
                                // Pass through only an id placement already holds.
                                id: r.id.unwrap_or_default(),
                                node: r.node_name,
                                health: r.health.as_str().to_string(),
                                lag_ms: r.lag.duration_ms,
                            })
                            .collect(),
                    }]
                };
                ContainerMapEntry {
                    namespace: ns.to_string(),
                    name: name.to_string(),
                    type_name: c.model.machine_name().to_string(),
                    shards,
                }
            })
            .collect::<Vec<_>>()
    };

    let migrations = node
        .jobs
        .list()
        .into_iter()
        .filter(|j| matches!(j.kind, JobKind::DataMigration))
        .map(|j| MigrationProgress {
            id: j.id.to_string(),
            status: if j.paused {
                "paused".into()
            } else {
                j.status.as_str().into()
            },
            bytes_copied: j.progress.bytes_copied,
            bytes_remaining: j.progress.bytes_remaining.unwrap_or(0),
        })
        .collect();

    TopologySnapshot {
        nodes,
        containers,
        migrations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spacestorage_placement::{ReplicaHealth, ReplicaLag};
    use std::time::Duration;
    use uuid::Uuid;

    #[test]
    fn describe_fills_lag_ms_without_minting_ids() {
        let mut snap = DescribeSnapshot::default();
        apply_placement_event(
            &mut snap,
            &PlacementEvent::ReplicaHealth {
                id: Uuid::nil(),
                origin: "n1".into(),
                namespace: "ns".into(),
                container: "c".into(),
                node_name: "n1".into(),
                health: ReplicaHealth::Behind,
                lag: ReplicaLag::new(Duration::from_millis(4200), 1),
                replica_id: None,
            },
        );
        let rows = snap.describe_container("ns", "c");
        assert_eq!(rows.len(), 1);
        assert!(rows[0].id.is_none());
        assert_eq!(rows[0].lag.duration_ms, 4200);
        assert_eq!(rows[0].health.as_str(), "behind");
    }
}
