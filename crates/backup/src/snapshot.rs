//! Snapshot create — per-drive LSN cut without pausing other drives (013 FR-013).

use crate::manifest::{
    ContainerSnapshotMeta, SnapshotManifest, SnapshotScope, SnapshotScopeKind,
};
use crate::BackupError;
use serde::{Deserialize, Serialize};
use spacestorage_storage::{ContentStore, StorageEngine, StorageMode};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SnapshotCreateRequest {
    pub data_dir: PathBuf,
    pub scope: SnapshotScope,
    /// Containers in scope with definitions (content pulled from engine).
    pub containers: Vec<ContainerSnapshotMeta>,
    /// Optional key material available at cut time (name → present).
    pub available_key_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContainerDataFile {
    id: Uuid,
    mode: String,
    /// Base64 not required — we store as JSON hex/base64 via serde_json map of strings.
    rows: HashMap<String, String>,
}

/// Create a snapshot under `{data_dir}/snapshots/<id>/`.
pub async fn create_snapshot(
    engine: &StorageEngine,
    req: SnapshotCreateRequest,
) -> Result<SnapshotManifest, BackupError> {
    // Fail closed if encrypted containers lack key refs at cut.
    for c in &req.containers {
        if let Some(ref kr) = c.key_ref {
            if !req.available_key_refs.iter().any(|k| k == kr) {
                return Err(BackupError::KeyRefMissing {
                    key_ref: kr.clone(),
                });
            }
        }
    }

    let snapshot_id = Uuid::now_v7();
    let root = req
        .data_dir
        .join("snapshots")
        .join(snapshot_id.to_string());
    tokio::fs::create_dir_all(&root)
        .await
        .map_err(|e| BackupError::Io(e.to_string()))?;

    // Record per-drive durable LSN independently (no cluster freeze).
    let drives = engine.all_drives().await;
    let mut positions = BTreeMap::new();
    for d in &drives {
        positions.insert(d.drive_id.clone(), d.durable_lsn());
    }
    if positions.is_empty() {
        positions.insert("default".into(), 0);
    }

    let content = engine.content.read().await;
    for c in &req.containers {
        let mode = parse_mode(&c.mode);
        let rows = if matches!(mode, StorageMode::Memory) {
            // Memory-mode content MUST be omitted.
            HashMap::new()
        } else {
            content.export_durable(c.id)
        };
        let file = ContainerDataFile {
            id: c.id,
            mode: c.mode.clone(),
            rows: encode_rows(&rows),
        };
        let path = root.join(format!("{}.json", c.id));
        let body = serde_json::to_vec_pretty(&file).map_err(|e| BackupError::Io(e.to_string()))?;
        tokio::fs::write(&path, body)
            .await
            .map_err(|e| BackupError::Io(e.to_string()))?;
    }
    drop(content);

    let manifest = SnapshotManifest::new(
        snapshot_id,
        req.scope,
        positions,
        req.containers,
    );
    let man_path = root.join("manifest.json");
    let man_body =
        serde_json::to_vec_pretty(&manifest).map_err(|e| BackupError::Io(e.to_string()))?;
    tokio::fs::write(&man_path, man_body)
        .await
        .map_err(|e| BackupError::Io(e.to_string()))?;

    Ok(manifest)
}

pub async fn load_manifest(data_dir: &Path, snapshot_id: Uuid) -> Result<SnapshotManifest, BackupError> {
    let path = data_dir
        .join("snapshots")
        .join(snapshot_id.to_string())
        .join("manifest.json");
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| BackupError::Io(e.to_string()))?;
    let man: SnapshotManifest =
        serde_json::from_slice(&bytes).map_err(|e| BackupError::Io(e.to_string()))?;
    man.validate().map_err(BackupError::Io)?;
    Ok(man)
}

pub async fn load_container_rows(
    data_dir: &Path,
    snapshot_id: Uuid,
    container_id: Uuid,
) -> Result<(StorageMode, HashMap<Vec<u8>, Vec<u8>>), BackupError> {
    let path = data_dir
        .join("snapshots")
        .join(snapshot_id.to_string())
        .join(format!("{container_id}.json"));
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| BackupError::Io(e.to_string()))?;
    let file: ContainerDataFile =
        serde_json::from_slice(&bytes).map_err(|e| BackupError::Io(e.to_string()))?;
    Ok((parse_mode(&file.mode), decode_rows(&file.rows)))
}

pub fn scope_container(namespace: &str, container: &str) -> SnapshotScope {
    SnapshotScope {
        kind: SnapshotScopeKind::Container,
        name: format!("{namespace}/{container}"),
    }
}

pub fn scope_namespace(namespace: &str) -> SnapshotScope {
    SnapshotScope {
        kind: SnapshotScopeKind::Namespace,
        name: namespace.into(),
    }
}

fn parse_mode(s: &str) -> StorageMode {
    match s {
        "memory" => StorageMode::Memory,
        "hybrid" => StorageMode::Hybrid,
        _ => StorageMode::Persistent,
    }
}

fn encode_rows(rows: &HashMap<Vec<u8>, Vec<u8>>) -> HashMap<String, String> {
    rows.iter()
        .map(|(k, v)| (hex::encode(k), hex::encode(v)))
        .collect()
}

fn decode_rows(rows: &HashMap<String, String>) -> HashMap<Vec<u8>, Vec<u8>> {
    rows.iter()
        .filter_map(|(k, v)| {
            let kk = hex::decode(k).ok()?;
            let vv = hex::decode(v).ok()?;
            Some((kk, vv))
        })
        .collect()
}

/// Convenience service wrapper used by admin / migrate.
pub struct SnapshotService {
    pub data_dir: PathBuf,
    /// When false, create returns BackupSlice10Required (first-binary admin path).
    pub slice10_enabled: bool,
}

impl SnapshotService {
    pub fn new(data_dir: PathBuf, slice10_enabled: bool) -> Self {
        Self {
            data_dir,
            slice10_enabled,
        }
    }

    pub async fn create(
        &self,
        engine: &StorageEngine,
        req: SnapshotCreateRequest,
    ) -> Result<SnapshotManifest, BackupError> {
        if !self.slice10_enabled {
            return Err(BackupError::BackupSlice10Required);
        }
        let mut req = req;
        req.data_dir = self.data_dir.clone();
        create_snapshot(engine, req).await
    }

    /// Position map from an existing snapshot (010 stores this, not a single LSN).
    pub async fn position_map(
        &self,
        snapshot_id: Uuid,
    ) -> Result<BTreeMap<String, u64>, BackupError> {
        if !self.slice10_enabled {
            return Err(BackupError::BackupSlice10Required);
        }
        let man = load_manifest(&self.data_dir, snapshot_id).await?;
        Ok(man.positions)
    }
}

/// Helper used in unit tests without a live engine — snapshot empty content store.
pub fn snapshot_content_omit_memory(
    content: &ContentStore,
    container_id: Uuid,
) -> HashMap<Vec<u8>, Vec<u8>> {
    content.export_durable(container_id)
}
