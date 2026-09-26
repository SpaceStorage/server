//! Size / connection / admission limits and process-local counters.

use crate::error::CompatError;
use crate::profile::DialectProfile;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;

/// Byte count used by size knobs.
pub type ByteCount = u64;

pub const DEFAULT_MAX_KEY: ByteCount = 1024; // 1 KiB
pub const DEFAULT_MAX_VALUE: ByteCount = 16 * 1024 * 1024; // 16 MiB
pub const DEFAULT_MAX_QUERY_TEXT: ByteCount = 1024 * 1024; // 1 MiB
pub const DEFAULT_MAX_RESULT: ByteCount = 64 * 1024 * 1024; // 64 MiB
pub const DEFAULT_MAX_CONNECTIONS_PER_ENTRYPOINT: u32 = 10_000;
pub const DEFAULT_MAX_CONNECTIONS_PER_PRINCIPAL: u32 = 1_000;

/// Concurrent / memory defaults live in `005` `query { }` but share LimitKind.
pub const DEFAULT_MAX_CONCURRENT_PER_NODE: u32 = 512;
pub const DEFAULT_MAX_CONCURRENT_PER_NAMESPACE: u32 = 128;
pub const DEFAULT_MAX_QUERY_MEMORY: ByteCount = 256 * 1024 * 1024; // 256 MiB

/// Named limit vocabulary for `limit_exceeded` / `admission_rejected`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LimitKind {
    Key,
    Value,
    QueryText,
    Result,
    ConnectionsEntrypoint,
    ConnectionsPrincipal,
    ConcurrentQueriesNode,
    ConcurrentQueriesNamespace,
    QueryMemory,
    Buffer,
    SpillDisk,
}

impl LimitKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Key => "key",
            Self::Value => "value",
            Self::QueryText => "query_text",
            Self::Result => "result",
            Self::ConnectionsEntrypoint => "connections_entrypoint",
            Self::ConnectionsPrincipal => "connections_principal",
            Self::ConcurrentQueriesNode => "concurrent_queries_node",
            Self::ConcurrentQueriesNamespace => "concurrent_queries_namespace",
            Self::QueryMemory => "query_memory",
            Self::Buffer => "buffer",
            Self::SpillDisk => "spill_disk",
        }
    }
}

/// Provenance of an effective limit value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LimitProvenance {
    BuiltIn,
    Configured,
}

/// Size and connection maxima (`limits { }`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    pub max_key: ByteCount,
    pub max_value: ByteCount,
    pub max_query_text: ByteCount,
    pub max_result: ByteCount,
    pub max_connections_per_entrypoint: u32,
    pub max_connections_per_principal: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_key: DEFAULT_MAX_KEY,
            max_value: DEFAULT_MAX_VALUE,
            max_query_text: DEFAULT_MAX_QUERY_TEXT,
            max_result: DEFAULT_MAX_RESULT,
            max_connections_per_entrypoint: DEFAULT_MAX_CONNECTIONS_PER_ENTRYPOINT,
            max_connections_per_principal: DEFAULT_MAX_CONNECTIONS_PER_PRINCIPAL,
        }
    }
}

impl Limits {
    /// Reject any zero knob (`limits_zero{knob}`).
    pub fn validate_nonzero(&self) -> Result<(), CompatError> {
        let checks: &[(&str, u64)] = &[
            ("max_key", self.max_key),
            ("max_value", self.max_value),
            ("max_query_text", self.max_query_text),
            ("max_result", self.max_result),
            (
                "max_connections_per_entrypoint",
                self.max_connections_per_entrypoint as u64,
            ),
            (
                "max_connections_per_principal",
                self.max_connections_per_principal as u64,
            ),
        ];
        for (knob, v) in checks {
            if *v == 0 {
                return Err(CompatError::limits_zero(*knob));
            }
        }
        Ok(())
    }

    pub fn check_key(&self, len: ByteCount) -> Result<(), CompatError> {
        check_size(LimitKind::Key, len, self.max_key)
    }

    pub fn check_value(&self, len: ByteCount) -> Result<(), CompatError> {
        check_size(LimitKind::Value, len, self.max_value)
    }

    pub fn check_query_text(&self, len: ByteCount) -> Result<(), CompatError> {
        check_size(LimitKind::QueryText, len, self.max_query_text)
    }

    pub fn check_result(&self, len: ByteCount) -> Result<(), CompatError> {
        check_size(LimitKind::Result, len, self.max_result)
    }

    pub fn check_connections_entrypoint(&self, current: u32) -> Result<(), CompatError> {
        if current < self.max_connections_per_entrypoint {
            Ok(())
        } else {
            Err(CompatError::limit_exceeded(
                LimitKind::ConnectionsEntrypoint.as_str(),
                current as u64,
                self.max_connections_per_entrypoint as u64,
            ))
        }
    }

    pub fn check_connections_principal(&self, current: u32) -> Result<(), CompatError> {
        if current < self.max_connections_per_principal {
            Ok(())
        } else {
            Err(CompatError::limit_exceeded(
                LimitKind::ConnectionsPrincipal.as_str(),
                current as u64,
                self.max_connections_per_principal as u64,
            ))
        }
    }
}

/// Hard size check — never truncate silently.
pub fn check_size(kind: LimitKind, current: ByteCount, max: ByteCount) -> Result<(), CompatError> {
    if current <= max {
        Ok(())
    } else {
        Err(CompatError::limit_exceeded(kind.as_str(), current, max))
    }
}

/// Spill mode for query-memory overage (FR-010).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpillMode {
    Off,
    On,
}

/// Admission policy owned by compat; knobs live in `005` `query { }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdmissionPolicy {
    pub spill: SpillMode,
    pub max_concurrent_per_node: u32,
    pub max_concurrent_per_namespace: u32,
    pub max_memory: ByteCount,
}

impl AdmissionPolicy {
    pub fn for_profile(profile: DialectProfile) -> Self {
        Self {
            spill: match profile {
                DialectProfile::FirstBinary | DialectProfile::HandlersComplete => SpillMode::Off,
                DialectProfile::CompleteProduct => SpillMode::On,
            },
            max_concurrent_per_node: DEFAULT_MAX_CONCURRENT_PER_NODE,
            max_concurrent_per_namespace: DEFAULT_MAX_CONCURRENT_PER_NAMESPACE,
            max_memory: DEFAULT_MAX_QUERY_MEMORY,
        }
    }

    /// Concurrent-query overflow always rejects; MUST NOT spill a slot.
    pub fn check_concurrent_node(&self, current: u32) -> Result<(), CompatError> {
        if current < self.max_concurrent_per_node {
            Ok(())
        } else {
            Err(CompatError::admission_rejected(
                LimitKind::ConcurrentQueriesNode.as_str(),
                current as u64,
                self.max_concurrent_per_node as u64,
            ))
        }
    }

    pub fn check_concurrent_namespace(&self, current: u32) -> Result<(), CompatError> {
        if current < self.max_concurrent_per_namespace {
            Ok(())
        } else {
            Err(CompatError::admission_rejected(
                LimitKind::ConcurrentQueriesNamespace.as_str(),
                current as u64,
                self.max_concurrent_per_namespace as u64,
            ))
        }
    }

    /// Memory overage: FB/HC always reject; CP may spill if `spillable`.
    ///
    /// Returns `Ok(true)` when the caller should spill; `Ok(false)` when under cap.
    pub fn check_memory(
        &self,
        reserved: ByteCount,
        additional: ByteCount,
        spillable: bool,
    ) -> Result<bool, CompatError> {
        let next = reserved.saturating_add(additional);
        if next <= self.max_memory {
            return Ok(false);
        }
        if self.spill == SpillMode::On && spillable {
            return Ok(true);
        }
        Err(CompatError::admission_rejected(
            LimitKind::QueryMemory.as_str(),
            next,
            self.max_memory,
        ))
    }
}

/// Process-local session counters (data-model §8). Not Raft; lost on restart.
///
/// Acquire order for queries: node concurrent → namespace concurrent → memory.
#[derive(Debug, Default)]
pub struct SessionCounters {
    entrypoint: Mutex<HashMap<String, AtomicU32>>,
    principal: Mutex<HashMap<String, AtomicU32>>,
    inflight_node: AtomicU32,
    inflight_ns: Mutex<HashMap<String, AtomicU32>>,
    reserved_memory: AtomicU64,
}

impl SessionCounters {
    pub fn new() -> Self {
        Self::default()
    }

    fn bump_map(map: &Mutex<HashMap<String, AtomicU32>>, key: &str) -> u32 {
        let mut g = map.lock().unwrap_or_else(|e| e.into_inner());
        let counter = g.entry(key.to_string()).or_insert_with(|| AtomicU32::new(0));
        counter.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn drop_map(map: &Mutex<HashMap<String, AtomicU32>>, key: &str) {
        let g = map.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(c) = g.get(key) {
            let _ = c.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                Some(v.saturating_sub(1))
            });
        }
    }

    fn peek_map(map: &Mutex<HashMap<String, AtomicU32>>, key: &str) -> u32 {
        let g = map.lock().unwrap_or_else(|e| e.into_inner());
        g.get(key)
            .map(|c| c.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    /// Accept a new TCP session on an entrypoint. Returns post-increment count.
    pub fn try_accept_entrypoint(
        &self,
        entrypoint: &str,
        limits: &Limits,
    ) -> Result<u32, CompatError> {
        let current = Self::peek_map(&self.entrypoint, entrypoint);
        limits.check_connections_entrypoint(current)?;
        Ok(Self::bump_map(&self.entrypoint, entrypoint))
    }

    pub fn release_entrypoint(&self, entrypoint: &str) {
        Self::drop_map(&self.entrypoint, entrypoint);
    }

    pub fn try_accept_principal(
        &self,
        principal_id: &str,
        limits: &Limits,
    ) -> Result<u32, CompatError> {
        let current = Self::peek_map(&self.principal, principal_id);
        limits.check_connections_principal(current)?;
        Ok(Self::bump_map(&self.principal, principal_id))
    }

    pub fn release_principal(&self, principal_id: &str) {
        Self::drop_map(&self.principal, principal_id);
    }

    /// Acquire query admission slots (node → namespace → memory). Never hangs.
    pub fn try_admit_query(
        &self,
        namespace: &str,
        policy: &AdmissionPolicy,
        memory: ByteCount,
        spillable: bool,
    ) -> Result<AdmissionGuard<'_>, CompatError> {
        let node_cur = self.inflight_node.load(Ordering::Relaxed);
        policy.check_concurrent_node(node_cur)?;
        let ns_cur = Self::peek_map(&self.inflight_ns, namespace);
        policy.check_concurrent_namespace(ns_cur)?;
        let reserved = self.reserved_memory.load(Ordering::Relaxed);
        let spill = policy.check_memory(reserved, memory, spillable)?;

        self.inflight_node.fetch_add(1, Ordering::Relaxed);
        Self::bump_map(&self.inflight_ns, namespace);
        self.reserved_memory
            .fetch_add(memory, Ordering::Relaxed);

        Ok(AdmissionGuard {
            counters: self,
            namespace: namespace.to_string(),
            memory,
            spill,
        })
    }

    pub fn inflight_queries_node(&self) -> u32 {
        self.inflight_node.load(Ordering::Relaxed)
    }

    pub fn reserved_query_memory(&self) -> u64 {
        self.reserved_memory.load(Ordering::Relaxed)
    }
}

/// RAII release for an admitted query.
pub struct AdmissionGuard<'a> {
    counters: &'a SessionCounters,
    namespace: String,
    memory: ByteCount,
    /// True when CP spill path should be used for this reservation.
    pub spill: bool,
}

impl Drop for AdmissionGuard<'_> {
    fn drop(&mut self) {
        let _ = self
            .counters
            .inflight_node
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                Some(v.saturating_sub(1))
            });
        SessionCounters::drop_map(&self.counters.inflight_ns, &self.namespace);
        let _ = self.counters.reserved_memory.fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            |v| Some(v.saturating_sub(self.memory)),
        );
    }
}

/// Effective limits with provenance for admin / CLI reporting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveLimits {
    pub limits: Limits,
    pub provenance: LimitProvenance,
}

impl EffectiveLimits {
    pub fn built_in() -> Self {
        Self {
            limits: Limits::default(),
            provenance: LimitProvenance::BuiltIn,
        }
    }

    pub fn configured(limits: Limits) -> Self {
        Self {
            limits,
            provenance: LimitProvenance::Configured,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let l = Limits::default();
        assert_eq!(l.max_key, 1024);
        assert_eq!(l.max_value, 16 * 1024 * 1024);
        assert_eq!(l.max_query_text, 1024 * 1024);
        assert_eq!(l.max_result, 64 * 1024 * 1024);
        assert_eq!(l.max_connections_per_entrypoint, 10_000);
        assert_eq!(l.max_connections_per_principal, 1_000);
        assert!(l.validate_nonzero().is_ok());
    }

    #[test]
    fn over_max_value() {
        let l = Limits::default();
        let err = l.check_value(l.max_value + 1).unwrap_err();
        match err {
            CompatError::LimitExceeded {
                limit,
                current,
                max,
            } => {
                assert_eq!(limit, "value");
                assert_eq!(current, l.max_value + 1);
                assert_eq!(max, l.max_value);
            }
            other => panic!("unexpected {other}"),
        }
    }

    #[test]
    fn over_key_query_result() {
        let l = Limits::default();
        assert!(matches!(
            l.check_key(l.max_key + 1),
            Err(CompatError::LimitExceeded { .. })
        ));
        assert!(matches!(
            l.check_query_text(l.max_query_text + 1),
            Err(CompatError::LimitExceeded { .. })
        ));
        assert!(matches!(
            l.check_result(l.max_result + 1),
            Err(CompatError::LimitExceeded { .. })
        ));
    }

    #[test]
    fn spill_defaults() {
        assert_eq!(
            AdmissionPolicy::for_profile(DialectProfile::FirstBinary).spill,
            SpillMode::Off
        );
        assert_eq!(
            AdmissionPolicy::for_profile(DialectProfile::CompleteProduct).spill,
            SpillMode::On
        );
    }

    #[test]
    fn concurrent_never_spills_slot() {
        let p = AdmissionPolicy::for_profile(DialectProfile::CompleteProduct);
        let err = p.check_concurrent_node(p.max_concurrent_per_node).unwrap_err();
        assert!(matches!(err, CompatError::AdmissionRejected { .. }));
    }

    #[test]
    fn fb_memory_rejects() {
        let p = AdmissionPolicy::for_profile(DialectProfile::FirstBinary);
        let err = p
            .check_memory(p.max_memory, 1, true)
            .unwrap_err();
        assert_eq!(
            err.code(),
            "admission_rejected"
        );
    }

    #[test]
    fn cp_spillable_memory() {
        let p = AdmissionPolicy::for_profile(DialectProfile::CompleteProduct);
        assert_eq!(p.check_memory(p.max_memory, 1, true).unwrap(), true);
        assert!(p.check_memory(p.max_memory, 1, false).is_err());
    }

    #[test]
    fn session_counters_connection_cap() {
        let c = SessionCounters::new();
        let mut lim = Limits::default();
        lim.max_connections_per_entrypoint = 2;
        assert!(c.try_accept_entrypoint("pg", &lim).is_ok());
        assert!(c.try_accept_entrypoint("pg", &lim).is_ok());
        assert!(c.try_accept_entrypoint("pg", &lim).is_err());
        c.release_entrypoint("pg");
        assert!(c.try_accept_entrypoint("pg", &lim).is_ok());
    }

    #[test]
    fn session_counters_admit() {
        let c = SessionCounters::new();
        let mut p = AdmissionPolicy::for_profile(DialectProfile::FirstBinary);
        p.max_concurrent_per_node = 1;
        let g = c
            .try_admit_query("ns", &p, 1024, false)
            .expect("admit");
        assert!(!g.spill);
        assert!(c.try_admit_query("ns", &p, 1024, false).is_err());
        drop(g);
        assert!(c.try_admit_query("ns", &p, 1024, false).is_ok());
    }
}
