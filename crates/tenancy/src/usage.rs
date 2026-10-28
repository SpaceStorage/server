//! In-memory quota usage ledger (not in cluster Raft).

use crate::error::{Result, TenancyError};
use crate::quotas::{QuotaSpec, QuotaUnit};
use crate::registry::NamespaceRegistry;
use parking_lot::Mutex;
use spacestorage_internode::QuotaDelta;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsageKey {
    pub namespace_id: Uuid,
    pub unit: QuotaUnit,
    pub datatype: Option<u64>, // hashed optional datatype; None = 0
}

type DeltaNotify = Arc<dyn Fn(QuotaDelta) + Send + Sync>;

#[derive(Default)]
pub struct QuotaUsage {
    inner: Mutex<HashMap<(Uuid, QuotaUnit, u64), u64>>,
    ops_buckets: Mutex<HashMap<(Uuid, u64), (u64, Instant, u64)>>,
    delta_notify: Mutex<Option<DeltaNotify>>,
    pub exceeded_total: AtomicU64,
    exceeded_by: Mutex<HashMap<(Uuid, QuotaUnit), u64>>,
}

impl QuotaUsage {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_delta_notify(&self, notify: Option<DeltaNotify>) {
        *self.delta_notify.lock() = notify;
    }

    fn dtype_key(dt: &Option<String>) -> u64 {
        match dt {
            None => 0,
            Some(s) => {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                s.hash(&mut h);
                h.finish()
            }
        }
    }

    pub fn get(&self, ns: Uuid, unit: QuotaUnit, datatype: &Option<String>) -> u64 {
        let k = (ns, unit, Self::dtype_key(datatype));
        *self.inner.lock().get(&k).unwrap_or(&0)
    }

    pub fn set(&self, ns: Uuid, unit: QuotaUnit, datatype: &Option<String>, value: u64) {
        let k = (ns, unit, Self::dtype_key(datatype));
        *self.inner.lock().entry(k).or_insert(0) = value;
    }

    pub fn add(&self, ns: Uuid, unit: QuotaUnit, datatype: &Option<String>, delta: i64) {
        let k = (ns, unit, Self::dtype_key(datatype));
        let mut g = self.inner.lock();
        let e = g.entry(k).or_insert(0);
        if delta >= 0 {
            *e = e.saturating_add(delta as u64);
        } else {
            *e = e.saturating_sub((-delta) as u64);
        }
    }

    fn record_exceeded(&self, ns: Uuid, unit: QuotaUnit) {
        self.exceeded_total.fetch_add(1, Ordering::Relaxed);
        let mut g = self.exceeded_by.lock();
        *g.entry((ns, unit)).or_insert(0) += 1;
    }

    /// Per `(namespace_id, unit)` reject counts for metrics exposition.
    pub fn exceeded_labeled(&self) -> Vec<(Uuid, QuotaUnit, u64)> {
        self.exceeded_by
            .lock()
            .iter()
            .map(|((ns, unit), count)| (*ns, *unit, *count))
            .collect()
    }

    fn emit_delta(&self, ns: Uuid, spec: &QuotaSpec, delta: i64) {
        if delta == 0 {
            return;
        }
        if let Some(notify) = self.delta_notify.lock().clone() {
            notify(QuotaDelta {
                namespace: ns,
                unit: spec.unit.as_str().into(),
                datatype: spec.datatype.clone(),
                delta,
            });
        }
    }

    /// Admit if under limit; increment on success. Never hangs.
    pub fn admit(&self, ns: Uuid, spec: &QuotaSpec, amount: u64) -> Result<()> {
        if spec.unit == QuotaUnit::OpsPerSec {
            return self.admit_ops_token(ns, spec);
        }
        let cur = self.get(ns, spec.unit, &spec.datatype);
        if spec.limit == 0 || cur >= spec.limit {
            self.record_exceeded(ns, spec.unit);
            return Err(TenancyError::QuotaExceeded {
                quota: spec.unit.as_str().into(),
                limit: spec.limit,
                usage: cur,
            });
        }
        if cur.saturating_add(amount) > spec.limit {
            self.record_exceeded(ns, spec.unit);
            return Err(TenancyError::QuotaExceeded {
                quota: spec.unit.as_str().into(),
                limit: spec.limit,
                usage: cur,
            });
        }
        self.add(ns, spec.unit, &spec.datatype, amount as i64);
        self.emit_delta(ns, spec, amount as i64);
        Ok(())
    }

    fn admit_ops_token(&self, ns: Uuid, spec: &QuotaSpec) -> Result<()> {
        let key = (ns, Self::dtype_key(&spec.datatype));
        let mut buckets = self.ops_buckets.lock();
        let now = Instant::now();
        let entry = buckets
            .entry(key)
            .or_insert((spec.limit, now, spec.limit));
        if now.duration_since(entry.1) >= Duration::from_secs(1) {
            entry.0 = entry.2.min(spec.limit);
            entry.1 = now;
        }
        entry.2 = spec.limit;
        if spec.limit == 0 || entry.0 == 0 {
            self.record_exceeded(ns, spec.unit);
            return Err(TenancyError::QuotaExceeded {
                quota: spec.unit.as_str().into(),
                limit: spec.limit,
                usage: spec.limit.saturating_sub(entry.0),
            });
        }
        entry.0 -= 1;
        drop(buckets);
        self.emit_delta(ns, spec, 1);
        Ok(())
    }

    /// Decrement usage after a successful admit (data-path failure, connection close, rollback).
    /// Publishes a negative internodes delta when `tenancy-quotas` is enabled.
    pub fn release_commit(&self, ns: Uuid, spec: &QuotaSpec, amount: u64) {
        if amount == 0 {
            return;
        }
        if spec.unit == QuotaUnit::OpsPerSec {
            let key = (ns, Self::dtype_key(&spec.datatype));
            let mut buckets = self.ops_buckets.lock();
            if let Some(entry) = buckets.get_mut(&key) {
                entry.0 = entry.0.saturating_add(amount).min(entry.2);
            }
            drop(buckets);
            #[cfg(feature = "tenancy-quotas")]
            self.emit_delta(ns, spec, -(amount as i64));
            return;
        }
        self.add(ns, spec.unit, &spec.datatype, -(amount as i64));
        #[cfg(feature = "tenancy-quotas")]
        self.emit_delta(ns, spec, -(amount as i64));
    }

    /// Apply a remote coordinator's best-effort additive delta (no re-publish).
    pub fn apply_delta(&self, delta: &QuotaDelta) -> Result<()> {
        let unit = QuotaUnit::parse(&delta.unit)?;
        if unit == QuotaUnit::OpsPerSec {
            // Best-effort: treat remote ops as consuming one token worth of pressure.
            let key = (delta.namespace, Self::dtype_key(&delta.datatype));
            let mut buckets = self.ops_buckets.lock();
            if let Some(entry) = buckets.get_mut(&key) {
                entry.0 = entry.0.saturating_sub(delta.delta.unsigned_abs());
            }
            return Ok(());
        }
        self.add(delta.namespace, unit, &delta.datatype, delta.delta);
        Ok(())
    }

    fn clear_bytes_objects(&self, ns: Uuid) {
        let mut g = self.inner.lock();
        g.retain(|(id, unit, _), _| {
            *id != ns || (*unit != QuotaUnit::Bytes && *unit != QuotaUnit::Objects)
        });
    }

    /// Seed bytes/objects from catalog logical totals (one copy; RF does not multiply).
    /// Every registry namespace is set (including zero); stale per-type counters are dropped.
    /// `connections` and other units are untouched.
    pub fn reconcile_from_catalog(
        &self,
        registry: &NamespaceRegistry,
        containers: impl Iterator<Item = (String, String, u64, u64)>,
    ) {
        let mut per_ns: HashMap<Uuid, (u64, u64, HashMap<String, (u64, u64)>)> = HashMap::new();
        for (namespace, type_name, bytes, objects) in containers {
            let Some(record) = registry.get_by_name(&namespace) else {
                continue;
            };
            let entry = per_ns.entry(record.id).or_default();
            entry.0 = entry.0.saturating_add(bytes);
            entry.1 = entry.1.saturating_add(objects);
            let type_entry = entry.2.entry(type_name).or_insert((0, 0));
            type_entry.0 = type_entry.0.saturating_add(bytes);
            type_entry.1 = type_entry.1.saturating_add(objects);
        }
        for record in registry.list() {
            if record.deleted {
                continue;
            }
            let (bytes, objects, per_type) = per_ns
                .get(&record.id)
                .cloned()
                .unwrap_or((0, 0, HashMap::new()));
            self.clear_bytes_objects(record.id);
            self.set(record.id, QuotaUnit::Bytes, &None, bytes);
            self.set(record.id, QuotaUnit::Objects, &None, objects);
            for (type_name, (tb, to)) in per_type {
                self.set(
                    record.id,
                    QuotaUnit::Bytes,
                    &Some(type_name.clone()),
                    tb,
                );
                self.set(record.id, QuotaUnit::Objects, &Some(type_name), to);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_internode_delta_without_replica_factor() {
        let usage = QuotaUsage::new();
        let namespace = Uuid::from_u128(7);
        usage
            .apply_delta(&QuotaDelta {
                namespace,
                unit: "bytes".into(),
                datatype: None,
                delta: 11,
            })
            .unwrap();
        assert_eq!(usage.get(namespace, QuotaUnit::Bytes, &None), 11);
    }

    #[test]
    fn exceeded_labeled_tracks_namespace_and_unit() {
        let usage = QuotaUsage::new();
        let ns = Uuid::from_u128(3);
        let spec = QuotaSpec {
            unit: QuotaUnit::Connections,
            limit: 0,
            datatype: None,
        };
        assert!(usage.admit(ns, &spec, 1).is_err());
        let labeled = usage.exceeded_labeled();
        assert_eq!(labeled.len(), 1);
        assert_eq!(labeled[0], (ns, QuotaUnit::Connections, 1));
    }

    #[test]
    fn reconcile_zeros_empty_namespace_and_drops_stale_types() {
        use crate::registry::NamespaceRegistry;
        use spacestorage_authz::Verb;

        let usage = QuotaUsage::new();
        let mut registry = NamespaceRegistry::default();
        let empty = registry
            .create(Verb::CLUSTER_ADMIN.bits(), "empty")
            .unwrap();
        let used = registry
            .create(Verb::CLUSTER_ADMIN.bits(), "used")
            .unwrap();
        usage.set(used.id, QuotaUnit::Bytes, &None, 99);
        usage.set(used.id, QuotaUnit::Objects, &Some("redis".into()), 5);
        usage.set(used.id, QuotaUnit::Connections, &None, 3);

        usage.reconcile_from_catalog(
            &registry,
            [(
                "used".into(),
                "redis".into(),
                10u64,
                2u64,
            )]
            .into_iter(),
        );

        assert_eq!(usage.get(empty.id, QuotaUnit::Bytes, &None), 0);
        assert_eq!(usage.get(empty.id, QuotaUnit::Objects, &None), 0);
        assert_eq!(usage.get(used.id, QuotaUnit::Bytes, &None), 10);
        assert_eq!(usage.get(used.id, QuotaUnit::Objects, &None), 2);
        assert_eq!(usage.get(used.id, QuotaUnit::Objects, &Some("redis".into())), 2);
        assert_eq!(usage.get(used.id, QuotaUnit::Objects, &Some("gone".into())), 0);
        assert_eq!(usage.get(used.id, QuotaUnit::Connections, &None), 3);
    }

    #[test]
    fn ops_per_sec_token_bucket_refills_each_second() {
        let usage = QuotaUsage::new();
        let ns = Uuid::from_u128(9);
        let spec = QuotaSpec {
            unit: QuotaUnit::OpsPerSec,
            limit: 2,
            datatype: None,
        };
        usage.admit(ns, &spec, 1).unwrap();
        usage.admit(ns, &spec, 1).unwrap();
        assert!(usage.admit(ns, &spec, 1).is_err());
        std::thread::sleep(Duration::from_millis(1100));
        usage.admit(ns, &spec, 1).unwrap();
    }
}
