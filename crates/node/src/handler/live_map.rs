//! Live cluster map snapshot from 011 membership, catalog, 010 jobs (009 T051).
//! Replica ids are never minted here — only reused when placement already exposes them.

use crate::Node;
use spacestorage_admin_ui::{
    ContainerMapEntry, MigrationProgress, NodeMapEntry, ReplicaMapEntry, ShardMapEntry,
    TopologySnapshot,
};
use spacestorage_membership::events::MemberStatus;
use spacestorage_migrate::JobKind;
use std::sync::Arc;

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
            ready: matches!(
                node.state.get(),
                crate::lifecycle::NodeState::Ready
            ),
        });
    }

    let containers = {
        let cat = node.catalog.read().unwrap();
        cat.list()
            .map(|c| {
                // Do not invent replica/shard ids — empty shards until 004 describe is mounted.
                let _ = ReplicaMapEntry {
                    id: String::new(),
                    node: String::new(),
                    health: String::new(),
                    lag_ms: 0,
                };
                ContainerMapEntry {
                    namespace: c.namespace.as_str().to_string(),
                    name: c.name.as_str().to_string(),
                    type_name: c.model.machine_name().to_string(),
                    shards: Vec::<ShardMapEntry>::new(),
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
