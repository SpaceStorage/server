use crate::Node;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NodeState {
    Starting = 0,
    Ready = 1,
    Draining = 2,
    Failed = 3,
}

impl NodeState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Draining => "draining",
            Self::Failed => "failed",
        }
    }

    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Ready,
            2 => Self::Draining,
            3 => Self::Failed,
            _ => Self::Starting,
        }
    }
}

pub struct NodeStateMachine {
    state: AtomicU8,
}

impl NodeStateMachine {
    pub fn new() -> Self {
        Self {
            state: AtomicU8::new(NodeState::Starting as u8),
        }
    }

    pub fn get(&self) -> NodeState {
        NodeState::from_u8(self.state.load(Ordering::SeqCst))
    }

    pub fn set(&self, s: NodeState) {
        self.state.store(s as u8, Ordering::SeqCst)
    }

    pub fn try_ready(&self) -> bool {
        self.state
            .compare_exchange(
                NodeState::Starting as u8,
                NodeState::Ready as u8,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok()
    }

    pub fn begin_drain(&self) -> bool {
        let cur = self.get();
        if matches!(cur, NodeState::Ready | NodeState::Starting) {
            self.set(NodeState::Draining);
            true
        } else {
            false
        }
    }
}

impl Default for NodeStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

/// Open drive WALs, restore catalog + content, retain engine on the node (013 US2 / T061).
pub async fn restore_storage(node: &Node) -> Result<(), String> {
    let cfg = node.config.load();
    let data_dir = cfg.storage_data_dir.as_ref().map(PathBuf::from);
    let drives: Vec<(String, PathBuf)> = cfg
        .storage
        .drives
        .iter()
        .map(|d| (d.id.clone(), PathBuf::from(&d.path)))
        .collect();
    // No durable drives configured — memory-only / lab without data_dir is OK.
    if data_dir.is_none() && drives.is_empty() {
        info!("no storage.data_dir / drives; skipping WAL restore");
        return Ok(());
    }
    let sync = match cfg.storage.sync.as_deref() {
        Some(s) => spacestorage_storage::SyncMode::parse(s).map_err(|e| e.to_string())?,
        None => spacestorage_storage::SyncMode::Fdatasync,
    };
    let max_wait = Duration::from_millis(cfg.storage.group_commit_max_wait_ms.unwrap_or(2));
    let max_bytes = cfg.storage.group_commit_max_bytes.unwrap_or(1024 * 1024) as usize;
    let metrics = Arc::new(spacestorage_storage::WalMetrics::default());
    let engine = Arc::new(spacestorage_storage::StorageEngine::new(Arc::clone(&metrics)));
    engine
        .open_drives(
            data_dir.as_deref(),
            &drives,
            sync,
            max_wait,
            max_bytes,
            &cfg.node_name,
        )
        .await
        .map_err(|e| e.to_string())?;
    // Catalog restore: disk definitions (if present) + in-memory Node catalog modes.
    let mut catalog_entries =
        spacestorage_storage::restore::restore_catalog(data_dir.as_deref())
            .await
            .map_err(|e| e.to_string())?;
    {
        let cat = node.catalog.read().map_err(|e| e.to_string())?;
        for c in cat.list() {
            let mode = match c.mode {
                spacestorage_types::StorageModeChoice::Memory => {
                    spacestorage_storage::StorageMode::Memory
                }
                spacestorage_types::StorageModeChoice::Persistent => {
                    spacestorage_storage::StorageMode::Persistent
                }
                spacestorage_types::StorageModeChoice::Hybrid => {
                    spacestorage_storage::StorageMode::Hybrid
                }
            };
            catalog_entries.push(spacestorage_storage::restore::CatalogEntry {
                container_id: c.id,
                mode,
                key_ref: None,
            });
        }
    }
    let checkpoints = std::collections::HashMap::new();
    // 014 KeyAuthority seam: resolve container data keys before WAL content apply (T065).
    let keys = resolve_container_wal_keys(node, &catalog_entries).await;
    let report = spacestorage_storage::restore::restore_before_ready(
        &engine,
        &catalog_entries,
        &checkpoints,
        &metrics,
        &keys,
    )
    .await
    .map_err(|e| e.to_string())?;
    if *engine.node_degraded.read().await {
        warn!(
            records = report.records,
            applied = report.applied,
            "storage restore complete with degraded drives"
        );
    } else {
        info!(
            records = report.records,
            applied = report.applied,
            memory_cleared = report.memory_cleared,
            catalog = report.catalog_definitions,
            "storage restore complete"
        );
    }
    *node.storage.write().await = Some(engine);
    Ok(())
}

/// Node seam for `014` `KeyAuthority` during WAL restore (T065).
/// Returns resolved data keys for encrypted catalog entries; missing authority → Unavailable.
async fn resolve_container_wal_keys(
    node: &Node,
    catalog: &[spacestorage_storage::restore::CatalogEntry],
) -> spacestorage_storage::restore::WalDataKeys {
    let authority = node.key_authority().await;
    spacestorage_storage::restore::resolve_wal_data_keys(
        catalog,
        authority.as_deref(),
    )
    .await
}

/// Begin graceful drain: refuse new **tenant** accepts via `is_draining`; keep admin /
/// admin-http accept loops alive so status/reload-reject stay reachable (FR-006 / FR-015 / T088).
/// Does **not** force-cancel in-flight handlers yet (T073); does **not** cancel listener
/// shutdown token (`cancel`) — that happens after the drain period in `Node::run`.
pub fn request_drain(node: &Node) -> bool {
    node.drain_flag
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let started = node.state.begin_drain();
    if started {
        info!(state = "draining", "drain requested");
        // Wake `Node::run` without tearing down listeners.
        node.drain_started.cancel();
    }
    started
}

/// Abort remaining in-flight work immediately (drain timeout or second signal).
pub fn force_abort(node: &Node) {
    warn!("forcing cancel of in-flight handlers");
    node.force_cancel.cancel();
}

fn handle_signal(node: &Arc<Node>) {
    match node.state.get() {
        NodeState::Draining => {
            warn!("second stop signal during drain; aborting immediately");
            force_abort(node);
        }
        NodeState::Starting => {
            info!("stop signal before ready; aborting startup");
            node.drain_flag
                .store(true, std::sync::atomic::Ordering::SeqCst);
            node.state.begin_drain();
            node.drain_started.cancel();
            node.cancel.cancel();
            node.force_cancel.cancel();
        }
        NodeState::Ready => {
            request_drain(node);
        }
        NodeState::Failed => {}
    }
}

/// Watch SIGTERM / SIGINT for drain / second-signal abort.
pub async fn watch_os_signals(node: Arc<Node>) {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = match signal(SignalKind::terminate()) {
            Ok(s) => s,
            Err(e) => {
                warn!(error=%e, "cannot install SIGTERM handler");
                return;
            }
        };
        let mut sigint = match signal(SignalKind::interrupt()) {
            Ok(s) => s,
            Err(e) => {
                warn!(error=%e, "cannot install SIGINT handler");
                return;
            }
        };
        loop {
            tokio::select! {
                _ = sigterm.recv() => handle_signal(&node),
                _ = sigint.recv() => handle_signal(&node),
            }
        }
    }
    #[cfg(not(unix))]
    {
        loop {
            if tokio::signal::ctrl_c().await.is_err() {
                return;
            }
            handle_signal(&node);
        }
    }
}
