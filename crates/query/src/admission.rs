//! Admission limits + tokens (005).

use crate::error::ExecError;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

pub const DEFAULT_MAX_CONCURRENT_PER_NODE: u32 = 512;
pub const DEFAULT_MAX_CONCURRENT_PER_NAMESPACE: u32 = 128;
pub const DEFAULT_MAX_MEMORY: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct AdmissionLimits {
    pub max_concurrent_per_node: u32,
    pub max_concurrent_per_namespace: u32,
    pub max_memory: u64,
    pub spill: bool,
}

impl Default for AdmissionLimits {
    fn default() -> Self {
        Self {
            max_concurrent_per_node: DEFAULT_MAX_CONCURRENT_PER_NODE,
            max_concurrent_per_namespace: DEFAULT_MAX_CONCURRENT_PER_NAMESPACE,
            max_memory: DEFAULT_MAX_MEMORY,
            spill: false,
        }
    }
}

#[derive(Debug, Default)]
struct AdmissionState {
    node_in_flight: u32,
    ns_in_flight: HashMap<String, u32>,
    memory_reserved: u64,
}

#[derive(Debug, Clone)]
pub struct AdmissionController {
    limits: AdmissionLimits,
    state: Arc<Mutex<AdmissionState>>,
}

impl AdmissionController {
    pub fn new(limits: AdmissionLimits) -> Self {
        Self {
            limits,
            state: Arc::new(Mutex::new(AdmissionState::default())),
        }
    }

    /// Acquire concurrent slots then memory. `spillable` (sort/hash/agg) MAY
    /// proceed past the memory cap when spill is on; concurrent overflow NEVER
    /// uses spill (FR-010).
    pub fn try_acquire(&self, namespace: &str, memory: u64) -> Result<AdmissionToken, ExecError> {
        self.try_acquire_spillable(namespace, memory, false)
    }

    pub fn try_acquire_spillable(
        &self,
        namespace: &str,
        memory: u64,
        spillable: bool,
    ) -> Result<AdmissionToken, ExecError> {
        let mut st = self.state.lock();
        // Concurrent-query caps hard-reject; spill MUST NOT admit an extra slot.
        if st.node_in_flight >= self.limits.max_concurrent_per_node {
            return Err(ExecError::Admission {
                limit: "concurrent_queries_node".into(),
                current: st.node_in_flight as u64,
                max: self.limits.max_concurrent_per_node as u64,
            });
        }
        let ns_cur = *st.ns_in_flight.get(namespace).unwrap_or(&0);
        if ns_cur >= self.limits.max_concurrent_per_namespace {
            return Err(ExecError::Admission {
                limit: "concurrent_queries_namespace".into(),
                current: ns_cur as u64,
                max: self.limits.max_concurrent_per_namespace as u64,
            });
        }
        let next = st.memory_reserved.saturating_add(memory);
        let over_memory = next > self.limits.max_memory;
        if over_memory {
            if self.limits.spill && spillable {
                // Spill path: admit without reserving past-cap memory accounting
                // (spill dir holds the working set on disk).
            } else {
                return Err(ExecError::Admission {
                    limit: "query_memory".into(),
                    current: next,
                    max: self.limits.max_memory,
                });
            }
        }
        st.node_in_flight += 1;
        *st.ns_in_flight.entry(namespace.to_string()).or_insert(0) += 1;
        if !over_memory {
            st.memory_reserved = st.memory_reserved.saturating_add(memory);
        }
        Ok(AdmissionToken {
            state: Arc::clone(&self.state),
            namespace: namespace.to_string(),
            memory: if over_memory { 0 } else { memory },
            spilled: over_memory && self.limits.spill && spillable,
        })
    }

    pub fn limits(&self) -> &AdmissionLimits {
        &self.limits
    }
}

#[derive(Debug)]
pub struct AdmissionToken {
    state: Arc<Mutex<AdmissionState>>,
    namespace: String,
    memory: u64,
    /// True when this query was admitted via spill past the memory cap.
    pub spilled: bool,
}

impl Drop for AdmissionToken {
    fn drop(&mut self) {
        let mut st = self.state.lock();
        st.node_in_flight = st.node_in_flight.saturating_sub(1);
        if let Some(c) = st.ns_in_flight.get_mut(&self.namespace) {
            *c = c.saturating_sub(1);
        }
        st.memory_reserved = st.memory_reserved.saturating_sub(self.memory);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spillable_over_memory_when_spill_on() {
        let c = AdmissionController::new(AdmissionLimits {
            max_concurrent_per_node: 8,
            max_concurrent_per_namespace: 8,
            max_memory: 1024,
            spill: true,
        });
        let t = c.try_acquire_spillable("ns", 4096, true).unwrap();
        assert!(t.spilled);
        // Concurrent overflow still rejects even with spill.
        let c2 = AdmissionController::new(AdmissionLimits {
            max_concurrent_per_node: 1,
            max_concurrent_per_namespace: 8,
            max_memory: 1024 * 1024,
            spill: true,
        });
        let _hold = c2.try_acquire("ns", 1).unwrap();
        let err = c2.try_acquire_spillable("ns", 1, true).unwrap_err();
        assert!(
            matches!(err, ExecError::Admission { limit, .. } if limit == "concurrent_queries_node")
        );
    }

    #[test]
    fn non_spillable_over_memory_rejects_naming_cap() {
        let c = AdmissionController::new(AdmissionLimits {
            max_concurrent_per_node: 8,
            max_concurrent_per_namespace: 8,
            max_memory: 1024,
            spill: true,
        });
        let err = c.try_acquire_spillable("ns", 4096, false).unwrap_err();
        match err {
            ExecError::Admission { limit, .. } => assert_eq!(limit, "query_memory"),
            other => panic!("{other:?}"),
        }
    }
}
