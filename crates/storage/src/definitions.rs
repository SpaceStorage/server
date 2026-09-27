//! Persist container definitions for boot restore (`{data_dir}/catalog/definitions.jsonl`).

use crate::error::StorageError;
use crate::mode::StorageMode;
use serde_json::json;
use spacestorage_types::L3Model;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct PersistedDefinition {
    pub id: Uuid,
    pub namespace: String,
    pub name: String,
    pub model: L3Model,
    pub mode: StorageMode,
}

fn path(data_dir: &Path) -> PathBuf {
    data_dir.join("catalog").join("definitions.jsonl")
}

pub async fn append_definition(
    data_dir: &Path,
    def: &PersistedDefinition,
) -> Result<(), StorageError> {
    let p = path(data_dir);
    let line = format!(
        "{}\n",
        json!({
            "id": def.id.to_string(),
            "namespace": def.namespace,
            "name": def.name,
            "model": def.model.machine_name(),
            "mode": match def.mode {
                StorageMode::Memory => "memory",
                StorageMode::Persistent => "persistent",
                StorageMode::Hybrid => "hybrid",
            },
        })
    );
    let p2 = p.clone();
    let line2 = line.clone();
    tokio::task::spawn_blocking(move || {
        if let Some(parent) = p2.parent() {
            std::fs::create_dir_all(parent)?;
        }
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&p2)?;
        f.write_all(line2.as_bytes())?;
        f.sync_all()?;
        Ok::<(), std::io::Error>(())
    })
    .await
    .map_err(|e| StorageError::Join(e.to_string()))?
    .map_err(|e| StorageError::Io(e))?;
    Ok(())
}

pub async fn load_definitions(data_dir: &Path) -> Result<Vec<PersistedDefinition>, StorageError> {
    let p = path(data_dir);
    let p2 = p.clone();
    let bytes = tokio::task::spawn_blocking(move || {
        if !p2.exists() {
            return Ok::<Option<Vec<u8>>, std::io::Error>(None);
        }
        Ok(Some(std::fs::read(&p2)?))
    })
    .await
    .map_err(|e| StorageError::Join(e.to_string()))??;
    let Some(bytes) = bytes else {
        return Ok(Vec::new());
    };
    let text = String::from_utf8_lossy(&bytes);
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(line)
            .map_err(|e| StorageError::WalCorrupt(format!("catalog parse: {e}")))?;
        let id = v
            .get("id")
            .and_then(|x| x.as_str())
            .ok_or_else(|| StorageError::WalCorrupt("catalog missing id".into()))?;
        let container_id = Uuid::parse_str(id)
            .map_err(|e| StorageError::WalCorrupt(format!("catalog uuid: {e}")))?;
        let mode = match v.get("mode").and_then(|x| x.as_str()).unwrap_or("persistent") {
            "memory" => StorageMode::Memory,
            "hybrid" => StorageMode::Hybrid,
            _ => StorageMode::Persistent,
        };
        let model = v
            .get("model")
            .and_then(|x| x.as_str())
            .and_then(L3Model::parse)
            .unwrap_or(L3Model::KvStore);
        out.push(PersistedDefinition {
            id: container_id,
            namespace: v
                .get("namespace")
                .and_then(|x| x.as_str())
                .unwrap_or("demo")
                .into(),
            name: v
                .get("name")
                .and_then(|x| x.as_str())
                .unwrap_or("data")
                .into(),
            model,
            mode,
        });
    }
    Ok(out)
}
