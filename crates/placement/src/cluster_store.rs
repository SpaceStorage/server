//! Placement cluster-log events (data-model §17 / plan R3).
//!
//! The placement crate owns the event types and describe apply. Raft shipping is
//! provided by `006` `RaftClusterStore` on the node — this crate MUST NOT depend
//! on controlplane. Schemas and container definitions stay off this log (FR-003).

use crate::replica::{DescribeSnapshot, Replica, ReplicaHealth, ReplicaLag};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Append-only placement event. T099 ships `ReplicaHealth`; other variants land with T008.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlacementEvent {
    /// Update replica health + lag in the describe snapshot every node resolves.
    ReplicaHealth {
        /// Event id (uuid v7 when minted by callers); used for idempotent apply.
        id: Uuid,
        origin: String,
        namespace: String,
        container: String,
        node_name: String,
        health: ReplicaHealth,
        #[serde(default)]
        lag: ReplicaLag,
        /// Existing replica id only — never minted here.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replica_id: Option<String>,
    },
}

/// Apply a placement event into the describe snapshot (idempotent by event fields).
pub fn apply_placement_event(snap: &mut DescribeSnapshot, event: &PlacementEvent) {
    match event {
        PlacementEvent::ReplicaHealth {
            namespace,
            container,
            node_name,
            health,
            lag,
            replica_id,
            ..
        } => {
            snap.upsert_replica(
                namespace.clone(),
                container.clone(),
                Replica {
                    node_name: node_name.clone(),
                    health: *health,
                    lag: lag.clone(),
                    id: replica_id.clone(),
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn replica_health_updates_describe_snapshot() {
        let mut snap = DescribeSnapshot::default();
        let ev = PlacementEvent::ReplicaHealth {
            id: Uuid::nil(),
            origin: "n1".into(),
            namespace: "ns".into(),
            container: "kv".into(),
            node_name: "n2".into(),
            health: ReplicaHealth::Behind,
            lag: ReplicaLag::new(Duration::from_millis(900), 2),
            replica_id: None,
        };
        apply_placement_event(&mut snap, &ev);
        let rows = snap.describe_container("ns", "kv");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].node_name, "n2");
        assert_eq!(rows[0].lag.duration_ms, 900);
        assert_eq!(rows[0].health, ReplicaHealth::Behind);
        assert!(rows[0].id.is_none());
    }
}
