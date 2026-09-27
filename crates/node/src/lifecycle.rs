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
    if let Some(dir) = data_dir.as_ref() {
        if let Ok(defs) = spacestorage_storage::load_definitions(dir).await {
            for d in defs {
                if !catalog_entries
                    .iter()
                    .any(|e| e.container_id == d.id)
                {
                    catalog_entries.push(spacestorage_storage::restore::CatalogEntry {
                        container_id: d.id,
                        mode: d.mode,
                        key_ref: None,
                    });
                }
            }
        }
    }
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
            if !catalog_entries.iter().any(|e| e.container_id == c.id) {
                catalog_entries.push(spacestorage_storage::restore::CatalogEntry {
                    container_id: c.id,
                    mode,
                    key_ref: None,
                });
            }
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
    *node.storage.write().await = Some(Arc::clone(&engine));

    // Wire sync durable hooks so client puts wait for WAL fsync (constitution II / G6 residual).
    if let Some(dir) = data_dir.clone() {
        let eng_put = Arc::clone(&engine);
        let eng_create = Arc::clone(&engine);
        let dir_create = dir.clone();
        let put_hook: spacestorage_types::DurablePutHook = Arc::new(move |id, _mode, key, val| {
            let eng = Arc::clone(&eng_put);
            let key = key.to_string();
            let val = val.to_vec();
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async move {
                    eng.put_kv_durable(id, &key, &val)
                        .await
                        .map_err(|e| e.to_string())
                })
            })
        });
        let create_hook: spacestorage_types::DurableCreateHook =
            Arc::new(move |id, ns, name, model, mode| {
                let dir = dir_create.clone();
                let ns = ns.to_string();
                let name = name.to_string();
                let smode = match mode {
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
                let def = spacestorage_storage::PersistedDefinition {
                    id,
                    namespace: ns,
                    name,
                    model,
                    mode: smode,
                };
                let _ = &eng_create;
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async move {
                        spacestorage_storage::append_definition(&dir, &def)
                            .await
                            .map_err(|e| e.to_string())
                    })
                })
            });
        {
            let mut cat = node.catalog.write().map_err(|e| e.to_string())?;
            cat.set_durable_hooks(Some(put_hook), Some(create_hook));
        }
    }

    // Hydrate live catalog from persisted definitions + WAL content.
    if let Some(dir) = data_dir.as_ref() {
        let defs = spacestorage_storage::load_definitions(dir)
            .await
            .map_err(|e| e.to_string())?;
        if !defs.is_empty() {
            let content = engine.content.read().await;
            let mut cat = node.catalog.write().map_err(|e| e.to_string())?;
            for d in &defs {
                let mode = match d.mode {
                    spacestorage_storage::StorageMode::Memory => {
                        spacestorage_types::StorageModeChoice::Memory
                    }
                    spacestorage_storage::StorageMode::Persistent => {
                        spacestorage_types::StorageModeChoice::Persistent
                    }
                    spacestorage_storage::StorageMode::Hybrid => {
                        spacestorage_types::StorageModeChoice::Hybrid
                    }
                };
                let _ = cat.restore_container(d.id, &d.namespace, &d.name, d.model, mode, None);
                let exported = content.export_durable(d.id);
                let mut rows = std::collections::HashMap::new();
                for (k, v) in exported {
                    let key = String::from_utf8_lossy(&k).into_owned();
                    rows.insert(key, v);
                }
                let _ = cat.replace_rows(d.id, rows);
            }
        }
    }

    Ok(())
}

/// 011: start membership when cluster bootstrap, join, or name is configured.
/// Without a cluster block, first-binary lab tests may become ready without membership.
pub async fn start_membership(node: &Node) -> Result<(), String> {
    let cfg = node.config.load();
    let join_set = cfg.cluster.join.is_some();
    let needs = cfg.cluster.bootstrap || join_set || cfg.cluster.name.is_some();
    if !needs {
        info!("no cluster bootstrap/join/name; skipping membership");
        return Ok(());
    }
    let data_dir = cfg
        .storage_data_dir
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            // Ephemeral identity under a node-local temp-ish path derived from config path.
            node.config_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join("data")
        });
    let quorum_domain = cfg
        .cluster
        .quorum_domain
        .clone()
        .unwrap_or_else(|| "default".into());
    let ladder = if cfg.cluster.topology_ladder.is_empty() {
        vec!["az".into()]
    } else {
        cfg.cluster.topology_ladder.clone()
    };
    let svc = Arc::new(spacestorage_membership::MembershipService::new(
        spacestorage_membership::MembershipStartOpts {
            data_dir,
            node_name: cfg.node_name.clone(),
            cluster_name: cfg
                .cluster
                .name
                .clone()
                .unwrap_or_else(|| "cluster".into()),
            quorum_domain,
            token_file: cfg.cluster.token_file.as_ref().map(PathBuf::from),
            bootstrap: cfg.cluster.bootstrap,
            join: join_set,
            foreign_seeds: false,
            topology_ladder: ladder,
            labels: cfg.labels.clone(),
        },
    ));
    svc.on_start().await.map_err(|e| e.to_string())?;
    // T073: inject cluster join secret into fabric (empty↔empty must not succeed).
    // Bootstrap/restore: accepted epochs. Join mode: presented secret from token_file.
    if let Some(secret) = svc.accepted_join_secret() {
        node.fabric.set_join_secret(secret);
        info!("fabric: join secret injected from membership epochs");
    } else if svc.is_join_mode() {
        match svc.load_presented_secret().await {
            Ok(secret) => {
                node.fabric.set_join_secret(secret);
                info!("fabric: join secret injected from token_file");
            }
            Err(e) => {
                return Err(e.to_string());
            }
        }
    }
    *node.membership.write().await = Some(Arc::clone(&svc));

    // T054: after on_start in join mode, contact seeds before advertising ready.
    if svc.is_pending_only() {
        drive_first_join(node, &svc).await?;
    }
    Ok(())
}

/// Build seed list + JoinRequest and drive first-join (T054).
pub async fn drive_first_join(
    node: &Node,
    svc: &spacestorage_membership::MembershipService,
) -> Result<(), String> {
    let cfg = node.config.load();
    let seeds: Vec<spacestorage_membership::SeedEndpoint> = cfg
        .cluster
        .seeds
        .iter()
        .map(|s| spacestorage_membership::SeedEndpoint {
            name: s.name.clone(),
            address: s.address.clone(),
            port: s.port,
        })
        .collect();
    if seeds.is_empty() {
        warn!("membership: join mode with empty seeds — staying pending without contact");
        return Ok(());
    }
    let internodes_address = cfg
        .entrypoints
        .iter()
        .find(|e| e.handler == "internode")
        .map(|e| format!("{}:{}", e.address, e.port))
        .unwrap_or_else(|| "127.0.0.1:0".into());
    let join_token = if let Some(path) = cfg.cluster.join_token_file.as_ref() {
        let text = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| e.to_string())?;
        Some(
            uuid::Uuid::parse_str(text.trim())
                .map_err(|e| format!("join_token_file: {e}"))?,
        )
    } else {
        None
    };
    let ack = svc
        .drive_first_join(&seeds, cfg.labels.clone(), internodes_address, join_token)
        .await
        .map_err(|e| e.to_string())?;
    match ack {
        spacestorage_membership::JoinAck::Refused { code } => {
            Err(format!("join_refused:{code}"))
        }
        spacestorage_membership::JoinAck::Pending
        | spacestorage_membership::JoinAck::Admitted { .. }
        | spacestorage_membership::JoinAck::Replaced { .. } => {
            // Re-inject epochs after admit so fabric matches cluster secret epochs.
            if let Some(secret) = svc.accepted_join_secret() {
                node.fabric.set_join_secret(secret);
            }
            Ok(())
        }
    }
}

/// T053: while pending first-join, stay up (not Failed) and re-contact seeds until
/// admitted/token or cancel. Does not advertise ready until `may_become_ready()`.
pub async fn wait_pending_join(node: &Arc<Node>) -> Result<(), String> {
    loop {
        if node.cancel.is_cancelled() || node.is_draining() {
            return Ok(());
        }
        let (pending, ready) = {
            let g = node.membership.read().await;
            match g.as_ref() {
                Some(m) => (m.is_pending_only(), m.may_become_ready()),
                None => (false, true),
            }
        };
        if ready {
            return Ok(());
        }
        if !pending {
            // Not an in-progress first join — refuse ready without staying forever.
            return Err("membership_not_ready".into());
        }
        info!("membership: pending join — staying up without ready");
        // Re-drive JoinRequest so post-admit seed returns Admitted.
        if let Some(svc) = node.membership.read().await.clone() {
            let _ = drive_first_join(node, &svc).await;
        }
        if node
            .membership
            .read()
            .await
            .as_ref()
            .map(|m| m.may_become_ready())
            .unwrap_or(false)
        {
            return Ok(());
        }
        tokio::select! {
            _ = node.cancel.cancelled() => return Ok(()),
            _ = node.drain_started.cancelled() => return Ok(()),
            _ = tokio::time::sleep(Duration::from_secs(2)) => {}
        }
    }
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
