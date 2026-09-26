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

    pub fn try_acquire(
        &self,
        namespace: &str,
        memory: u64,
    ) -> Result<AdmissionToken, ExecError> {
        let mut st = self.state.lock();
        if st.node_in_flight >= self.limits.max_concurrent_per_node {
            return Err(ExecError::Admission {
                limit: "node".into(),
                current: st.node_in_flight as u64,
                max: self.limits.max_concurrent_per_node as u64,
            });
        }
        let ns_cur = *st.ns_in_flight.get(namespace).unwrap_or(&0);
        if ns_cur >= self.limits.max_concurrent_per_namespace {
            return Err(ExecError::Admission {
                limit: "namespace".into(),
                current: ns_cur as u64,
                max: self.limits.max_concurrent_per_namespace as u64,
            });
        }
        if !self.limits.spill && st.memory_reserved.saturating_add(memory) > self.limits.max_memory {
            return Err(ExecError::Admission {
                limit: "memory".into(),
                current: st.memory_reserved,
                max: self.limits.max_memory,
            });
        }
        st.node_in_flight += 1;
        *st.ns_in_flight.entry(namespace.to_string()).or_insert(0) += 1;
        st.memory_reserved = st.memory_reserved.saturating_add(memory);
        Ok(AdmissionToken {
            state: Arc::clone(&self.state),
            namespace: namespace.to_string(),
            memory,
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
