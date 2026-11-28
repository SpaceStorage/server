//! Node adapter for `spacestorage-handler-redis`.
//!
//! `017` T038: the total-order stream leadership lease is backed by the real
//! `crates/controlplane/src/lease.rs` primitives. The Redis handler only sees the
//! `StreamLeaseProvider` seam; this module owns the concrete store and mirrors a
//! granted lease into a live control plane best-effort.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use async_trait::async_trait;
use spacestorage_compat::LimitServices;
use spacestorage_handler_redis::{
    NodeInfoProvider, NodeInfoSnapshot, RedisHandler, StreamLeaseError, StreamLeaseProvider,
};
use spacestorage_types::ContainerCatalog;
use tokio_util::sync::CancellationToken;

use crate::handler::{client_dialect, ClientStream, Handler};

/// Default Redis key-space namespace (the handler default) used to resolve the
/// namespace the best-effort lease mirror targets.
const DEFAULT_REDIS_NAMESPACE: &str = "demo";

pub struct RedisPortHandler {
    inner: RedisHandler,
}

impl RedisPortHandler {
    pub fn new(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self::with_limits(catalog, LimitServices::built_in())
    }

    pub fn with_limits(
        catalog: Arc<RwLock<ContainerCatalog>>,
        limit_services: LimitServices,
    ) -> Self {
        Self {
            inner: RedisHandler::with_limits(catalog, limit_services)
                .with_profile(client_dialect()),
        }
    }

    pub fn with_tenancy(
        catalog: Arc<RwLock<ContainerCatalog>>,
        limit_services: LimitServices,
        tenancy: Arc<spacestorage_tenancy::Tenancy>,
    ) -> Self {
        Self {
            inner: RedisHandler::with_limits(catalog, limit_services)
                .with_tenancy(tenancy)
                .with_profile(client_dialect()),
        }
    }

    /// `017` T038: production wiring. Injects the `controlplane`-backed
    /// leadership-lease provider so total-order stream appends present the real
    /// `006` epoch and are fenced against `crates/controlplane/src/lease.rs`.
    pub fn with_controlplane(
        catalog: Arc<RwLock<ContainerCatalog>>,
        limit_services: LimitServices,
        controlplane: Arc<
            tokio::sync::RwLock<Option<Arc<spacestorage_controlplane::ControlPlane>>>,
        >,
        tenancy: Arc<spacestorage_tenancy::Tenancy>,
    ) -> Self {
        Self {
            inner: RedisHandler::with_limits(catalog, limit_services)
                .with_tenancy(tenancy)
                .with_profile(client_dialect())
                .with_stream_leases(Arc::new(ControlPlaneStreamLease::new(controlplane))),
        }
    }

    /// `017` US4 T051: inject this node's real facts for the Redis `INFO`
    /// renderer (`server`/`memory`/`replication`).
    pub fn with_node_info(mut self, info: Arc<dyn NodeInfoProvider>) -> Self {
        self.inner = self.inner.with_node_info(info);
        self
    }

    /// `017` US4 T054: the configured `max_block` cap, reported by
    /// `CONFIG GET max_block` and applied to blocking commands.
    pub fn with_max_block(mut self, max_block: Option<std::time::Duration>) -> Self {
        self.inner = self.inner.with_max_block(max_block);
        self
    }
}

/// `017` US4 T051: this node's real facts for the Redis `INFO` renderer.
///
/// SpaceStorage is not Redis replication, so `role` is `"master"` — this process
/// coordinates its own key-space in the single-node first product; a
/// membership/placement-derived role is a documented residual. Uptime is this
/// process's own wall-clock and `worker_threads` is the runtime's configured
/// count. Memory, port, and name have no internal equivalent here and are
/// omitted rather than fabricated (research R10, constitution X).
pub struct NodeRedisInfo {
    started: Instant,
    worker_threads: u32,
}

impl NodeRedisInfo {
    pub fn new(started: Instant, worker_threads: u32) -> Self {
        Self {
            started,
            worker_threads,
        }
    }
}

impl NodeInfoProvider for NodeRedisInfo {
    fn snapshot(&self) -> NodeInfoSnapshot {
        NodeInfoSnapshot {
            role: "master",
            uptime_seconds: self.started.elapsed().as_secs(),
            worker_threads: self.worker_threads,
            used_memory_bytes: None,
            tcp_port: None,
            node_name: None,
        }
    }
}

/// Per-stream-key leadership-lease store backed by `controlplane::lease`.
///
/// The epoch fence is *not* reimplemented here: `grant` mints a
/// `LeadershipLease` with `controlplane::lease::grant`, and `check` fences with
/// `controlplane::lease::check_epoch`. When no control plane is live the store
/// still fences locally (a missing lease is `NotGranted`, never "fall open"); a
/// successful grant is mirrored into a live control plane best-effort.
pub struct ControlPlaneStreamLease {
    controlplane:
        Arc<tokio::sync::RwLock<Option<Arc<spacestorage_controlplane::ControlPlane>>>>,
    holder: uuid::Uuid,
    leases:
        Mutex<BTreeMap<String, spacestorage_controlplane::LeadershipLease>>,
}

impl ControlPlaneStreamLease {
    pub fn new(
        controlplane: Arc<
            tokio::sync::RwLock<Option<Arc<spacestorage_controlplane::ControlPlane>>>,
        >,
    ) -> Self {
        Self {
            controlplane,
            holder: uuid::Uuid::new_v4(),
            leases: Mutex::new(BTreeMap::new()),
        }
    }
}

impl StreamLeaseProvider for ControlPlaneStreamLease {
    fn grant(&self, key: &str) -> Result<u64, StreamLeaseError> {
        let mut leases = self.leases.lock().expect("stream lease");
        if let Some(lease) = leases.get(key) {
            return Ok(lease.epoch);
        }
        // The workspace `uuid` has no v5, so derive a stable per-key container id
        // with a random v4 cached in the lease (stable for this node's lifetime).
        let container_id = uuid::Uuid::new_v4();
        let lease = spacestorage_controlplane::lease::grant(
            container_id,
            self.holder,
            None,
            1,
            "stream",
        )
        .map_err(|_| StreamLeaseError::NotGranted)?;
        leases.insert(key.to_string(), lease.clone());
        // Best-effort mirror into a live control plane: a missing/unstarted
        // control plane or an unresolvable namespace must not fail the append.
        if let Ok(guard) = self.controlplane.try_read() {
            if let Some(cp) = guard.as_ref() {
                if let Some(ns) = cp.tenancy_namespace_by_name(DEFAULT_REDIS_NAMESPACE) {
                    let _ = cp.put_lease(ns.id, lease.clone());
                }
            }
        }
        Ok(lease.epoch)
    }

    fn check(&self, key: &str, presented: u64) -> Result<(), StreamLeaseError> {
        let leases = self.leases.lock().expect("stream lease");
        let lease = leases.get(key).ok_or(StreamLeaseError::NotGranted)?;
        match spacestorage_controlplane::lease::check_epoch(presented, lease) {
            Ok(()) => Ok(()),
            Err(spacestorage_controlplane::Error::StaleEpoch { have, need }) => {
                Err(StreamLeaseError::Stale { have, need })
            }
            Err(_) => Err(StreamLeaseError::NotGranted),
        }
    }

    fn adopt(&self, key: &str, epoch: u64) {
        let lease = spacestorage_controlplane::LeadershipLease {
            container_id: uuid::Uuid::new_v4(),
            holder: self.holder,
            epoch,
            granted_index: 0,
        };
        self.leases
            .lock()
            .expect("stream lease")
            .insert(key.to_string(), lease);
    }
}

#[async_trait]
impl Handler for RedisPortHandler {
    fn name(&self) -> &str {
        "redis"
    }

    fn kind(&self) -> &str {
        "protocol"
    }

    fn owner(&self) -> &str {
        "02-protocol-drivers"
    }

    async fn serve(&self, stream: ClientStream, cancel: CancellationToken) {
        self.inner.serve(stream, cancel).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> ControlPlaneStreamLease {
        ControlPlaneStreamLease::new(Arc::new(tokio::sync::RwLock::new(None)))
    }

    #[test]
    fn grant_fences_stale_and_refuses_unknown() {
        let p = provider();
        assert_eq!(p.grant("s").unwrap(), 1);
        assert!(p.check("s", 1).is_ok());
        assert_eq!(
            p.check("s", 2),
            Err(StreamLeaseError::Stale { have: 2, need: 1 })
        );
        assert_eq!(p.check("unknown", 1), Err(StreamLeaseError::NotGranted));
    }

    #[test]
    fn adopt_restores_persisted_epoch() {
        let p = provider();
        p.adopt("s", 5);
        assert!(p.check("s", 5).is_ok());
        assert_eq!(
            p.check("s", 1),
            Err(StreamLeaseError::Stale { have: 1, need: 5 })
        );
    }
}
