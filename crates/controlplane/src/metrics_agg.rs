//! Shared-datatype metric aggregation on the **namespace** primary only (FR-013).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedMetricAggregate {
    pub namespace_id: Uuid,
    pub series: BTreeMap<String, f64>,
    pub stale: bool,
}

struct PeerPush {
    at: Instant,
    series: BTreeMap<String, f64>,
}

/// In-memory merge on namespace primary. Cluster primary MUST NOT aggregate.
pub struct MetricsAggregator {
    namespace_id: Uuid,
    heartbeat: Duration,
    peers: BTreeMap<Uuid, PeerPush>,
    local: BTreeMap<String, f64>,
}

impl MetricsAggregator {
    pub fn new(namespace_id: Uuid, heartbeat: Duration) -> Self {
        Self {
            namespace_id,
            heartbeat,
            peers: BTreeMap::new(),
            local: BTreeMap::new(),
        }
    }

    pub fn push_local(&mut self, name: impl Into<String>, value: f64) {
        self.local.insert(name.into(), value);
    }

    pub fn on_peer_push(&mut self, peer: Uuid, series: BTreeMap<String, f64>) {
        self.peers.insert(
            peer,
            PeerPush {
                at: Instant::now(),
                series,
            },
        );
    }

    pub fn snapshot(&self) -> SharedMetricAggregate {
        let mut series = self.local.clone();
        let mute = self.heartbeat.saturating_mul(2);
        let mut stale = false;
        for p in self.peers.values() {
            if p.at.elapsed() > mute {
                stale = true;
                continue;
            }
            for (k, v) in &p.series {
                *series.entry(k.clone()).or_insert(0.0) += *v;
            }
        }
        SharedMetricAggregate {
            namespace_id: self.namespace_id,
            series,
            stale,
        }
    }
}
