pub mod admin;
pub mod buffer;
pub mod effective;
pub mod entrypoint;
pub mod handler;
pub mod lifecycle;
pub mod logging;
pub mod reload;
pub mod runtime;
pub mod stats;

use crate::buffer::BufferRegistry;
use crate::effective::EffectiveStore;
use crate::handler::{admin_http, admin_tcp, echo, postgresql, redis, stub_cluster, HandlerRegistry};
use crate::lifecycle::{NodeState, NodeStateMachine};
use crate::stats::Stats;
use arc_swap::ArcSwap;
use spacestorage_config::NodeConfig;
use spacestorage_crypto::SharedKeyAuthority;
use spacestorage_storage::StorageEngine;
use spacestorage_types::ContainerCatalog;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::info;

pub struct Node {
    pub config_path: PathBuf,
    pub config: Arc<ArcSwap<NodeConfig>>,
    pub state: Arc<NodeStateMachine>,
    pub effective: Arc<EffectiveStore>,
    pub handlers: Arc<RwLock<HandlerRegistry>>,
    pub buffers: Arc<BufferRegistry>,
    pub stats: Arc<Stats>,
    /// First-binary L3 container catalog (create/describe/CRUD/drop before protocol wiring).
    pub catalog: Arc<RwLock<ContainerCatalog>>,
    /// Open storage engine after restore (013); held for runtime I/O.
    pub storage: tokio::sync::RwLock<Option<Arc<StorageEngine>>>,
    /// Optional `014` envelope KeyAuthority for WAL decrypt-on-restore (T065).
    key_authority: tokio::sync::RwLock<Option<SharedKeyAuthority>>,
    /// Cancelled when drain begins — wakes `run` (does **not** stop accept loops).
    pub drain_started: CancellationToken,
    /// Cancelled to stop accept loops (after drain period, startup abort, or test shutdown).
    pub cancel: CancellationToken,
    /// Cancelled after drain timeout or second signal — aborts in-flight handlers.
    pub force_cancel: CancellationToken,
    pub tasks: TaskTracker,
    pub reload_lock: Mutex<()>,
    pub started_at: Instant,
    pub worker_threads: u32,
    pub threads_source: String,
    pub drain_flag: Arc<AtomicBool>,
}

impl Node {
    pub fn is_draining(&self) -> bool {
        self.drain_flag.load(Ordering::SeqCst) || matches!(self.state.get(), NodeState::Draining)
    }

    /// Optional `014` KeyAuthority used when decrypting encrypted WAL payloads on restore.
    pub async fn key_authority(&self) -> Option<SharedKeyAuthority> {
        self.key_authority.read().await.clone()
    }

    /// Install envelope KeyAuthority (014) for WAL encrypt/decrypt seams.
    pub async fn set_key_authority(&self, auth: Option<SharedKeyAuthority>) {
        *self.key_authority.write().await = auth;
    }

    pub fn handler_names(&self) -> Vec<String> {
        self.handlers.read().unwrap().names()
    }

    pub fn boot_first_binary(
        config_path: PathBuf,
        cfg: NodeConfig,
        worker_threads: u32,
        threads_source: String,
    ) -> Arc<Self> {
        let buffers = BufferRegistry::from_config(&cfg);
        let effective = EffectiveStore::new(&cfg, worker_threads, &threads_source, vec![]);
        let node = Arc::new(Self {
            config_path,
            config: Arc::new(ArcSwap::from_pointee(cfg)),
            state: Arc::new(NodeStateMachine::new()),
            effective: Arc::new(effective),
            handlers: Arc::new(RwLock::new(HandlerRegistry::new())),
            buffers: Arc::new(buffers),
            stats: Arc::new(Stats::new(worker_threads)),
            catalog: Arc::new(RwLock::new(ContainerCatalog::new())),
            storage: tokio::sync::RwLock::new(None),
            key_authority: tokio::sync::RwLock::new(None),
            drain_started: CancellationToken::new(),
            cancel: CancellationToken::new(),
            force_cancel: CancellationToken::new(),
            tasks: TaskTracker::new(),
            reload_lock: Mutex::new(()),
            started_at: Instant::now(),
            worker_threads,
            threads_source,
            drain_flag: Arc::new(AtomicBool::new(false)),
        });
        {
            let mut reg = node.handlers.write().unwrap();
            reg.register(Arc::new(admin_tcp::AdminTcpHandler {
                node: Arc::clone(&node),
            }))
            .unwrap();
            reg.register(Arc::new(admin_http::AdminHttpHandler {
                node: Arc::clone(&node),
            }))
            .unwrap();
            reg.register(Arc::new(echo::EchoHandler)).unwrap();
            for name in ["internode", "replication"] {
                reg.register(Arc::new(stub_cluster::ClusterPortHandler { name }))
                    .unwrap();
            }
            // 002 first-binary: real async protocol handlers (replace byte-sink stubs).
            reg.register(Arc::new(postgresql::PostgresqlPortHandler::new(Arc::clone(
                &node.catalog,
            ))))
            .unwrap();
            reg.register(Arc::new(redis::RedisPortHandler::new(Arc::clone(
                &node.catalog,
            ))))
            .unwrap();
        }
        node
    }

    pub async fn run(self: Arc<Self>) -> Result<(), String> {
        let sig_node = self.clone();
        tokio::spawn(async move {
            lifecycle::watch_os_signals(sig_node).await;
        });

        // 013 US2: open per-drive WAL and restore before ready.
        if let Err(e) = lifecycle::restore_storage(&self).await {
            tracing::error!(error = %e, "storage restore failed");
            self.state.set(lifecycle::NodeState::Failed);
            return Err(e);
        }

        let handles = entrypoint::bind_all(self.clone()).await?;
        if self.is_draining() || self.cancel.is_cancelled() {
            // Stop before ready (FR-007): listeners already opened are dropped when
            // accept tasks see cancel; never enter ready.
            self.cancel.cancel();
            self.force_cancel.cancel();
            self.tasks.close();
            let _ = self.tasks.wait().await;
            for h in handles {
                let _ = h.await;
            }
            return Ok(());
        }
        if !self.state.try_ready() {
            return Ok(());
        }
        info!(state = "ready", "node ready");

        // Wait until drain is requested (stop / SIGTERM / SIGINT) or external cancel.
        // Drain does **not** cancel accept loops yet — admin stays reachable (FR-015 / T088).
        tokio::select! {
            _ = self.drain_started.cancelled() => {}
            _ = self.cancel.cancelled() => {}
        }

        let timeout = self.config.load().drain_timeout;
        self.tasks.close();
        let drained = tokio::time::timeout(timeout, self.tasks.wait()).await;
        if drained.is_err() {
            self.stats.mark_drain_timed_out();
            lifecycle::force_abort(&self);
            // Brief grace for handlers to notice force_cancel.
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(1),
                self.tasks.wait(),
            )
            .await;
        }
        // Drain period over — stop all accept loops (including admin) so the process can exit.
        self.cancel.cancel();
        for h in handles {
            let _ = h.await;
        }
        Ok(())
    }
}

pub fn first_binary_handler_names() -> Vec<&'static str> {
    vec![
        "admin",
        "admin-http",
        "internode",
        "replication",
        "postgresql",
        "redis",
        "echo",
    ]
}

pub fn complete_product_handler_names() -> Vec<&'static str> {
    let mut v = first_binary_handler_names();
    v.extend([
        "cassandra",
        "elasticsearch",
        "clickhouse",
        "clickhouse-http",
        "s3",
        "webdav",
    ]);
    v
}
