//! Node-introspection seam for the Redis `INFO` renderer (`017` US4 T051).
//!
//! `INFO` must express this node's real state (role, uptime, worker count), not a
//! fabricated stock-Redis-looking constant (research R10, constitution X). The
//! concrete node facts live in `crates/node`; the Redis handler consumes only
//! this seam so it stays free of a node/control-plane/storage dependency, exactly
//! as it consumes [`crate::lease::StreamLeaseProvider`].

use std::time::Instant;

/// A truthful snapshot of the node facts `INFO` can report. A `None` field means
/// "no internal equivalent" and the renderer omits it rather than faking it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeInfoSnapshot {
    /// This node's real role. SpaceStorage is not Redis replication, so the role
    /// is membership/placement-derived, never a hardcoded stock answer.
    pub role: &'static str,
    pub uptime_seconds: u64,
    pub worker_threads: u32,
    /// `None` ⇒ omit the `memory` byte fields (keep the `# Memory` header).
    pub used_memory_bytes: Option<u64>,
    /// `None` ⇒ omit `tcp_port`.
    pub tcp_port: Option<u16>,
    /// `None` ⇒ omit `node_name`.
    pub node_name: Option<String>,
}

/// Read-only provider for a [`NodeInfoSnapshot`]. Injected by the node; the
/// handler installs [`StandaloneNodeInfo`] by default.
pub trait NodeInfoProvider: Send + Sync {
    fn snapshot(&self) -> NodeInfoSnapshot;
}

/// The handler-local default: a single-process node with no Redis-replica link.
///
/// `role` is `"master"` because this process is the sole coordinator of its
/// key-space; uptime is this process's own wall-clock. `worker_threads` is `1`
/// (the handler-local default makes no claim about a runtime it cannot see);
/// memory, port, and name are unknown here and therefore omitted.
pub struct StandaloneNodeInfo {
    started: Instant,
    tcp_port: Option<u16>,
    node_name: Option<String>,
}

impl StandaloneNodeInfo {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            tcp_port: None,
            node_name: None,
        }
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.tcp_port = Some(port);
        self
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.node_name = Some(name.into());
        self
    }
}

impl Default for StandaloneNodeInfo {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeInfoProvider for StandaloneNodeInfo {
    fn snapshot(&self) -> NodeInfoSnapshot {
        NodeInfoSnapshot {
            role: "master",
            uptime_seconds: self.started.elapsed().as_secs(),
            worker_threads: 1,
            used_memory_bytes: None,
            tcp_port: self.tcp_port,
            node_name: self.node_name.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_reports_truthful_defaults() {
        let s = StandaloneNodeInfo::new().snapshot();
        assert_eq!(s.role, "master");
        assert_eq!(s.worker_threads, 1);
        // Fields with no internal equivalent are omitted, never faked.
        assert_eq!(s.used_memory_bytes, None);
        assert_eq!(s.tcp_port, None);
        assert_eq!(s.node_name, None);
    }

    #[test]
    fn optional_node_facts_are_carried_through() {
        let s = StandaloneNodeInfo::new()
            .with_port(6379)
            .with_name("node-1")
            .snapshot();
        assert_eq!(s.tcp_port, Some(6379));
        assert_eq!(s.node_name.as_deref(), Some("node-1"));
    }
}
