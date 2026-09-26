//! In-memory quota usage ledger (not in cluster Raft).

use crate::error::{Result, TenancyError};
use crate::quotas::{QuotaSpec, QuotaUnit};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsageKey {
    pub namespace_id: Uuid,
    pub unit: QuotaUnit,
    pub datatype: Option<u64>, // hashed optional datatype; None = 0
}

#[derive(Default)]
pub struct QuotaUsage {
    inner: Mutex<HashMap<(Uuid, QuotaUnit, u64), u64>>,
    pub exceeded_total: AtomicU64,
}

impl QuotaUsage {
    pub fn new() -> Self {
        Self::default()
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

    /// Admit if under limit; increment on success. Never hangs.
    pub fn admit(&self, ns: Uuid, spec: &QuotaSpec, amount: u64) -> Result<()> {
        let cur = self.get(ns, spec.unit, &spec.datatype);
        if cur >= spec.limit {
            self.exceeded_total.fetch_add(1, Ordering::Relaxed);
            return Err(TenancyError::QuotaExceeded {
                quota: spec.unit.as_str().into(),
                limit: spec.limit,
                usage: cur,
            });
        }
        if cur.saturating_add(amount) > spec.limit && spec.limit > 0 {
            // Best-effort hard reject on overshoot.
            self.exceeded_total.fetch_add(1, Ordering::Relaxed);
            return Err(TenancyError::QuotaExceeded {
                quota: spec.unit.as_str().into(),
                limit: spec.limit,
                usage: cur,
            });
        }
        if spec.limit == 0 {
            self.exceeded_total.fetch_add(1, Ordering::Relaxed);
            return Err(TenancyError::QuotaExceeded {
                quota: spec.unit.as_str().into(),
                limit: 0,
                usage: cur,
            });
        }
        self.add(ns, spec.unit, &spec.datatype, amount as i64);
        Ok(())
    }
}
