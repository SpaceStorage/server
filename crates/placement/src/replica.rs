//! Replica health states and describe snapshot (FB subset).
//!
//! Describe returns only replicas placement already holds — never mints ids (FR-024 / T098).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReplicaHealth {
    Empty,
    InSync,
    Behind,
    Unavailable,
    Moving,
    Removed,
    ContentLost,
}

impl ReplicaHealth {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::InSync => "in_sync",
            Self::Behind => "behind",
            Self::Unavailable => "unavailable",
            Self::Moving => "moving",
            Self::Removed => "removed",
            Self::ContentLost => "content_lost",
        }
    }
}

/// Async replication lag: wall duration + pending writes (data-model §9, FR-041).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplicaLag {
    /// Lag duration in milliseconds (serde-friendly stand-in for `Duration`).
    pub duration_ms: u64,
    pub pending_writes: u64,
}

impl ReplicaLag {
    pub fn new(duration: Duration, pending_writes: u64) -> Self {
        Self {
            duration_ms: duration.as_millis() as u64,
            pending_writes,
        }
    }

    pub fn duration(&self) -> Duration {
        Duration::from_millis(self.duration_ms)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Replica {
    pub node_name: String,
    pub health: ReplicaHealth,
    #[serde(default)]
    pub lag: ReplicaLag,
    /// Existing id when placement already holds one — never minted by describe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplicaSet {
    pub replicas: Vec<Replica>,
}

/// One described replica row (node, health, lag; optional existing id only).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplicaDescribe {
    pub namespace: String,
    pub container: String,
    pub node_name: String,
    pub health: ReplicaHealth,
    pub lag: ReplicaLag,
    pub id: Option<String>,
}

/// In-memory describe snapshot updated by `PlacementEvent::ReplicaHealth` (T098 / T099).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DescribeSnapshot {
    /// `"namespace\\0container"` → node_name → replica.
    #[serde(default)]
    replicas: BTreeMap<String, BTreeMap<String, Replica>>,
}

fn container_key(namespace: &str, container: &str) -> String {
    format!("{namespace}\0{container}")
}

impl DescribeSnapshot {
    pub fn is_empty(&self) -> bool {
        self.replicas.is_empty()
    }

    /// Upsert a replica row placement already holds (does not invent ids).
    pub fn upsert_replica(
        &mut self,
        namespace: impl Into<String>,
        container: impl Into<String>,
        replica: Replica,
    ) {
        let ns = namespace.into();
        let c = container.into();
        let key = container_key(&ns, &c);
        self.replicas
            .entry(key)
            .or_default()
            .insert(replica.node_name.clone(), replica);
    }

    /// Describe replicas for one container — empty when placement holds nothing.
    pub fn describe_container(&self, namespace: &str, container: &str) -> Vec<ReplicaDescribe> {
        let key = container_key(namespace, container);
        let Some(by_node) = self.replicas.get(&key) else {
            return Vec::new();
        };
        by_node
            .values()
            .map(|r| ReplicaDescribe {
                namespace: namespace.to_string(),
                container: container.to_string(),
                node_name: r.node_name.clone(),
                health: r.health,
                lag: r.lag.clone(),
                // Pass through only an id placement already holds — never mint.
                id: r.id.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lag_round_trip_duration() {
        let lag = ReplicaLag::new(Duration::from_millis(4200), 7);
        assert_eq!(lag.duration_ms, 4200);
        assert_eq!(lag.pending_writes, 7);
        assert_eq!(lag.duration(), Duration::from_millis(4200));
    }

    #[test]
    fn describe_empty_when_no_rows() {
        let snap = DescribeSnapshot::default();
        assert!(snap.describe_container("ns", "c").is_empty());
    }

    #[test]
    fn describe_returns_held_replicas_without_minting_ids() {
        let mut snap = DescribeSnapshot::default();
        snap.upsert_replica(
            "ns",
            "c",
            Replica {
                node_name: "n1".into(),
                health: ReplicaHealth::Behind,
                lag: ReplicaLag::new(Duration::from_millis(1500), 3),
                id: None,
            },
        );
        snap.upsert_replica(
            "ns",
            "c",
            Replica {
                node_name: "n2".into(),
                health: ReplicaHealth::InSync,
                lag: ReplicaLag::default(),
                id: Some("already-held".into()),
            },
        );
        let rows = snap.describe_container("ns", "c");
        assert_eq!(rows.len(), 2);
        let n1 = rows.iter().find(|r| r.node_name == "n1").unwrap();
        assert!(n1.id.is_none());
        assert_eq!(n1.lag.duration_ms, 1500);
        assert_eq!(n1.health.as_str(), "behind");
        let n2 = rows.iter().find(|r| r.node_name == "n2").unwrap();
        assert_eq!(n2.id.as_deref(), Some("already-held"));
    }
}
