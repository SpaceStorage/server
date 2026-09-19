pub mod admin;
pub mod buffer;
pub mod effective;
pub mod entrypoint;
pub mod handler;
pub mod lifecycle;
pub mod reload;
pub mod runtime;
pub mod stats;

use crate::buffer::BufferRegistry;
use crate::effective::EffectiveStore;
use crate::handler::{admin_http, admin_tcp, echo, stub_cluster, HandlerRegistry};
use crate::lifecycle::{NodeState, NodeStateMachine};
use crate::stats::Stats;
use arc_swap::ArcSwap;
use spacestorage_config::NodeConfig;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::info;

pub struct Node {
    pub config_path: PathBuf,
    pub config: Arc<ArcSwap<NodeConfig>>,
    pub state: Arc<NodeStateMachine>,
    pub effective: Arc<EffectiveStore>,
    pub handlers: Arc<RwLock<HandlerRegistry>>,
    pub buffers: Arc<BufferRegistry>,
    pub stats: Arc<Stats>,
    pub cancel: CancellationToken,
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
            cancel: CancellationToken::new(),
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
            for name in ["internode", "replication", "postgresql", "redis"] {
                reg.register(Arc::new(stub_cluster::ClusterPortHandler { name }))
                    .unwrap();
            }
        }
        node
    }

    pub async fn run(self: Arc<Self>) -> Result<(), String> {
        let handles = entrypoint::bind_all(self.clone()).await?;
        self.state.try_ready();
        info!(state = "ready", "node ready");
        self.cancel.cancelled().await;
        let timeout = self.config.load().drain_timeout;
        let drain = tokio::time::timeout(timeout, async {
            for h in handles {
                let _ = h.await;
            }
        })
        .await;
        if drain.is_err() {
            self.stats.mark_drain_timed_out();
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
