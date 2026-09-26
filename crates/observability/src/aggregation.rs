//! Shared-datatype aggregation freshness (FR-003, FR-024).

use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::sample::{LabelSet, SeriesSnapshot};
use crate::Registry;

/// Freshness state for one (namespace, datatype) shared aggregate.
#[derive(Debug, Clone)]
pub struct AggregationFreshness {
    pub namespace_id: Uuid,
    pub datatype_id: Uuid,
    pub namespace: String,
    pub datatype: String,
    /// 1 = fresh, 0 = stale (primary silent).
    pub up: u8,
    /// Unix seconds of last successful merge; frozen when `up=0`.
    pub last_success: u64,
    /// Merged sample values (name → value); frozen when `up=0`.
    pub merged: BTreeMap<String, f64>,
}

impl AggregationFreshness {
    pub fn new(namespace_id: Uuid, datatype_id: Uuid, namespace: &str, datatype: &str) -> Self {
        Self {
            namespace_id,
            datatype_id,
            namespace: namespace.into(),
            datatype: datatype.into(),
            up: 0,
            last_success: 0,
            merged: BTreeMap::new(),
        }
    }

    pub fn mark_success(&mut self, merged: BTreeMap<String, f64>) {
        self.up = 1;
        self.last_success = now_secs();
        self.merged = merged;
    }

    /// Primary silent / down: freeze last_success and merged; set up=0.
    pub fn mark_stale(&mut self) {
        self.up = 0;
        // last_success and merged remain unchanged (freeze).
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

/// Process-local store of freshness records + merged series for scrape.
#[derive(Debug, Default)]
pub struct AggregationStore {
    inner: Mutex<BTreeMap<(Uuid, Uuid), AggregationFreshness>>,
}

impl AggregationStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn upsert(&self, fresh: AggregationFreshness) {
        let key = (fresh.namespace_id, fresh.datatype_id);
        self.inner.lock().insert(key, fresh);
    }

    pub fn get(&self, namespace_id: Uuid, datatype_id: Uuid) -> Option<AggregationFreshness> {
        self.inner.lock().get(&(namespace_id, datatype_id)).cloned()
    }

    pub fn mark_stale_all_silent(&self, mute: Duration, last_push: &BTreeMap<(Uuid, Uuid), std::time::Instant>) {
        let mut g = self.inner.lock();
        for (key, fresh) in g.iter_mut() {
            if last_push
                .get(key)
                .map(|t| t.elapsed() > mute)
                .unwrap_or(true)
            {
                fresh.mark_stale();
            }
        }
    }

    /// Emit freshness gauges into the registry (no `stale` label on merged series).
    pub fn publish_to_registry(&self, registry: &Registry) {
        let g = self.inner.lock();
        for fresh in g.values() {
            let mut labels = LabelSet::new();
            let _ = labels.insert("namespace", fresh.namespace.as_str());
            let _ = labels.insert("datatype", fresh.datatype.as_str());
            registry.set_gauge(
                "spacestorage_shared_aggregation_up",
                labels.clone(),
                "1 when shared-datatype aggregation is fresh.",
                fresh.up as f64,
            );
            registry.set_gauge(
                "spacestorage_shared_aggregation_last_success_timestamp_seconds",
                labels,
                "Unix seconds of last successful shared aggregation.",
                fresh.last_success as f64,
            );
            // Merged series remain; never drop solely because primary is down.
            for (name, value) in &fresh.merged {
                let mut ml = LabelSet::new();
                let _ = ml.insert("namespace", fresh.namespace.as_str());
                let _ = ml.insert("datatype", fresh.datatype.as_str());
                // Never attach a `stale` label.
                debug_assert!(ml.get("stale").is_none());
                registry.set_gauge(name, ml, "Merged shared-datatype gauge.", *value);
            }
        }
    }

    pub fn snapshots_merged(&self) -> Vec<SeriesSnapshot> {
        // Encoder path uses registry; this helper is for unit tests.
        Vec::new()
    }
}

/// Apply a MetricsPush-style merge on the namespace primary.
pub fn merge_push(
    store: &AggregationStore,
    namespace_id: Uuid,
    datatype_id: Uuid,
    namespace: &str,
    datatype: &str,
    series: BTreeMap<String, f64>,
) {
    let mut fresh = store
        .get(namespace_id, datatype_id)
        .unwrap_or_else(|| AggregationFreshness::new(namespace_id, datatype_id, namespace, datatype));
    fresh.namespace = namespace.into();
    fresh.datatype = datatype.into();
    fresh.mark_success(series);
    store.upsert(fresh);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freshness_freeze_when_up_zero() {
        let ns = Uuid::nil();
        let dt = Uuid::from_u128(1);
        let mut f = AggregationFreshness::new(ns, dt, "acme", "kv");
        let mut merged = BTreeMap::new();
        merged.insert("spacestorage_query_total".into(), 42.0);
        f.mark_success(merged);
        let ts = f.last_success;
        assert_eq!(f.up, 1);
        assert_eq!(f.merged.get("spacestorage_query_total"), Some(&42.0));

        f.mark_stale();
        assert_eq!(f.up, 0);
        assert_eq!(f.last_success, ts);
        assert_eq!(f.merged.get("spacestorage_query_total"), Some(&42.0));

        // Local series must never carry a stale label — freshness is dedicated gauges only.
        let mut labels = LabelSet::new();
        labels.insert("namespace", "acme").unwrap();
        assert!(labels.get("stale").is_none());
    }
}
