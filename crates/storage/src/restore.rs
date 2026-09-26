//! Boot restore: catalog + WAL replay into content before node ready (013 US2 / T061 / T065).

use crate::checkpoint::stub_checkpoint;
use crate::content::{ContentStore, UnavailableReason};
use crate::error::StorageError;
use crate::mode::StorageMode;
use crate::wal::{maybe_decrypt_payload, wal_payload_aad, WalRecord, WalRecordType};
use crate::{StorageEngine, WalMetrics};
use spacestorage_crypto::KeyAuthority;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

pub struct RestoreReport {
    pub records: usize,
    pub applied: usize,
    pub skipped_unavailable: usize,
    pub memory_cleared: bool,
    pub catalog_definitions: usize,
    pub duration: std::time::Duration,
}

/// Minimal catalog definition row restored before content replay.
#[derive(Debug, Clone)]
pub struct CatalogEntry {
    pub container_id: Uuid,
    pub mode: StorageMode,
    /// When set, WAL payloads for this container are AEAD-encrypted (`014` key ref).
    pub key_ref: Option<String>,
}

/// Resolved WAL data keys for encrypted containers (T065).
///
/// - Container **absent** from the map → unencrypted (plaintext payload).
/// - Present with `Some(key)` → decrypt with that data key.
/// - Present with `None` → encrypted but key missing/unreadable → `Unavailable`.
pub type WalDataKeys = HashMap<Uuid, Option<[u8; 32]>>;

/// Resolve data keys for catalog entries that declare `key_ref` via `014` [`KeyAuthority`].
/// Unresolvable / missing authority → `None` entry (restore marks `Unavailable`).
pub async fn resolve_wal_data_keys(
    catalog: &[CatalogEntry],
    authority: Option<&dyn KeyAuthority>,
) -> WalDataKeys {
    let mut out = WalDataKeys::new();
    for e in catalog {
        let Some(key_ref) = e.key_ref.as_deref() else {
            continue;
        };
        match authority {
            None => {
                out.insert(e.container_id, None);
            }
            Some(auth) => match auth.resolve(key_ref).await {
                Ok(material) => {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(material.as_ref());
                    out.insert(e.container_id, Some(arr));
                }
                Err(_) => {
                    out.insert(e.container_id, None);
                }
            },
        }
    }
    out
}

/// Load container definitions from `{data_dir}/catalog/definitions.jsonl` when present.
/// Each line: `{"id":"<uuid>","mode":"persistent"|"memory"|"hybrid"[,"key_ref":"..."]}`.
pub async fn restore_catalog(
    data_dir: Option<&Path>,
) -> Result<Vec<CatalogEntry>, StorageError> {
    let Some(dir) = data_dir else {
        return Ok(Vec::new());
    };
    let path = dir.join("catalog").join("definitions.jsonl");
    let path_clone = path.clone();
    let bytes = tokio::task::spawn_blocking(move || {
        if !path_clone.exists() {
            return Ok::<Option<Vec<u8>>, std::io::Error>(None);
        }
        Ok(Some(std::fs::read(&path_clone)?))
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
        let mode = v
            .get("mode")
            .and_then(|x| x.as_str())
            .unwrap_or("persistent");
        let container_id = Uuid::parse_str(id)
            .map_err(|e| StorageError::WalCorrupt(format!("catalog uuid: {e}")))?;
        let mode = match mode {
            "memory" => StorageMode::Memory,
            "hybrid" => StorageMode::Hybrid,
            _ => StorageMode::Persistent,
        };
        let key_ref = v
            .get("key_ref")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        out.push(CatalogEntry {
            container_id,
            mode,
            key_ref,
        });
    }
    Ok(out)
}

fn decode_put_payload(payload: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    if payload.len() < 4 {
        return None;
    }
    let klen = u32::from_le_bytes(payload[0..4].try_into().ok()?) as usize;
    if 4 + klen > payload.len() {
        return None;
    }
    let key = payload[4..4 + klen].to_vec();
    let value = payload[4 + klen..].to_vec();
    Some((key, value))
}

fn apply_plaintext(content: &mut ContentStore, rec: &WalRecord, payload: &[u8]) {
    match rec.ty {
        WalRecordType::Put => {
            if let Some((k, v)) = decode_put_payload(payload) {
                content.put(rec.container_id, k, v);
            } else {
                // Raw payload (tests / non-kv): treat whole blob as key "".
                content.put(rec.container_id, Vec::new(), payload.to_vec());
            }
        }
        WalRecordType::Delete => {
            content.delete(rec.container_id, payload);
        }
        WalRecordType::TxnMarker => {}
    }
}

/// Decrypt (when required) then apply one WAL record. Never stores ciphertext as content.
fn apply_record(
    content: &mut ContentStore,
    drive_id: &str,
    rec: &WalRecord,
    keys: &WalDataKeys,
) -> bool {
    if content.is_unavailable(rec.container_id) {
        return false;
    }

    let data_key = match keys.get(&rec.container_id) {
        None => None,
        Some(None) => {
            content.mark_unavailable(rec.container_id, UnavailableReason::KeyUnresolvable);
            return false;
        }
        Some(Some(k)) => Some(k),
    };

    let aad = wal_payload_aad(drive_id, rec.lsn, rec.container_id);
    let plaintext = match maybe_decrypt_payload(&rec.payload, data_key, &aad) {
        Ok(pt) => pt,
        Err(_) => {
            content.mark_unavailable(rec.container_id, UnavailableReason::DecryptFailed);
            return false;
        }
    };
    apply_plaintext(content, rec, &plaintext);
    true
}

/// Catalog restore → open WALs (caller) → replay `lsn > checkpoint` into engine content.
/// Memory-mode content stays empty; hybrid hot set is rebuilt from WAL.
///
/// `keys`: see [`WalDataKeys`]. Encrypted containers with missing keys become `Unavailable`.
pub async fn restore_before_ready(
    engine: &StorageEngine,
    catalog: &[CatalogEntry],
    checkpoints: &HashMap<String, u64>,
    metrics: &WalMetrics,
    keys: &WalDataKeys,
) -> Result<RestoreReport, StorageError> {
    let start = Instant::now();
    {
        let mut content = engine.content.write().await;
        for e in catalog {
            content.register_mode(e.container_id, e.mode);
            if e.key_ref.is_some() && !keys.contains_key(&e.container_id) {
                // Encrypted in catalog but no resolve result → Unavailable.
                content.mark_unavailable(e.container_id, UnavailableReason::KeyUnresolvable);
            }
        }
        content.clear_memory_mode();
    }

    let drives = engine.all_drives().await;
    let mut applied = 0usize;
    let mut records = 0usize;
    let mut skipped_unavailable = 0usize;

    // Parallel per-drive replay on the blocking-friendly async path (DriveWal::replay_all).
    for d in &drives {
        let covered = checkpoints
            .get(&d.drive_id)
            .copied()
            .unwrap_or_else(|| stub_checkpoint(&d.drive_id, 0).covered_lsn);
        let recs = match d.replay_all().await {
            Ok(r) => r,
            Err(StorageError::WalCorrupt(_)) => {
                // Quarantine already attempted inside replay; mark degraded and continue.
                engine.mark_corrupt(&d.drive_id, "wal mid-log corrupt").await;
                continue;
            }
            Err(e) => return Err(e),
        };
        records += recs.len();
        let mut content = engine.content.write().await;
        for rec in recs {
            if rec.lsn <= covered {
                continue;
            }
            if content.is_unavailable(rec.container_id) {
                skipped_unavailable += 1;
                continue;
            }
            if apply_record(&mut content, &d.drive_id, &rec, keys) {
                applied += 1;
            } else if content.is_unavailable(rec.container_id) {
                skipped_unavailable += 1;
            }
        }
    }

    {
        let mut content = engine.content.write().await;
        content.clear_memory_mode();
    }

    let duration = start.elapsed();
    metrics
        .wal_recovery_duration_ns
        .fetch_add(duration.as_nanos() as u64, std::sync::atomic::Ordering::Relaxed);
    metrics
        .recovery_duration_ns
        .fetch_add(duration.as_nanos() as u64, std::sync::atomic::Ordering::Relaxed);
    metrics
        .recovery_total
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    Ok(RestoreReport {
        records,
        applied,
        skipped_unavailable,
        memory_cleared: true,
        catalog_definitions: catalog.len(),
        duration,
    })
}

/// Convenience: restore catalog from disk then apply WAL into `engine` (plaintext keys).
pub async fn restore_engine_before_ready(
    engine: &Arc<StorageEngine>,
    data_dir: Option<&Path>,
    metrics: &WalMetrics,
) -> Result<RestoreReport, StorageError> {
    let catalog = restore_catalog(data_dir).await?;
    let checkpoints = HashMap::new();
    let keys = WalDataKeys::new();
    restore_before_ready(engine, &catalog, &checkpoints, metrics, &keys).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wal::{SyncMode, WalRecordType};
    use crate::WalMetrics;
    use crate::content::UnavailableReason;
    use std::time::Duration;
    use tempfile::tempdir;

    fn kv_payload(key: &[u8], val: &[u8]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&(key.len() as u32).to_le_bytes());
        payload.extend_from_slice(key);
        payload.extend_from_slice(val);
        payload
    }

    #[tokio::test]
    async fn apply_wal_into_content_and_skip_memory() {
        let dir = tempdir().unwrap();
        let metrics = Arc::new(WalMetrics::default());
        let engine = Arc::new(StorageEngine::new(Arc::clone(&metrics)));
        engine
            .open_drives(
                Some(dir.path()),
                &[],
                SyncMode::Fdatasync,
                Duration::from_millis(2),
                1024 * 1024,
                "n1",
            )
            .await
            .unwrap();
        let wal = engine.get("default").await.unwrap();
        let persistent = Uuid::new_v4();
        let memory = Uuid::new_v4();
        let hybrid = Uuid::new_v4();

        let payload = kv_payload(b"key", b"val");
        wal.append_durable(persistent, 1, WalRecordType::Put, payload.clone(), None)
            .await
            .unwrap();
        wal.append_durable(memory, 2, WalRecordType::Put, payload.clone(), None)
            .await
            .unwrap();
        wal.append_durable(hybrid, 3, WalRecordType::Put, payload, None)
            .await
            .unwrap();

        let catalog = vec![
            CatalogEntry {
                container_id: persistent,
                mode: StorageMode::Persistent,
                key_ref: None,
            },
            CatalogEntry {
                container_id: memory,
                mode: StorageMode::Memory,
                key_ref: None,
            },
            CatalogEntry {
                container_id: hybrid,
                mode: StorageMode::Hybrid,
                key_ref: None,
            },
        ];
        let report =
            restore_before_ready(&engine, &catalog, &HashMap::new(), &metrics, &WalDataKeys::new())
                .await
                .unwrap();
        assert!(report.applied >= 2);
        assert!(report.memory_cleared);
        let content = engine.content.read().await;
        assert_eq!(content.get(persistent, b"key"), Some(b"val".as_slice()));
        assert_eq!(content.len(memory), 0);
        assert_eq!(content.get(hybrid, b"key"), Some(b"val".as_slice()));
        assert_eq!(content.hybrid_hot_len(hybrid), 1);
    }

    #[tokio::test]
    async fn decrypt_encrypted_wal_on_restore() {
        let dir = tempdir().unwrap();
        let metrics = Arc::new(WalMetrics::default());
        let engine = Arc::new(StorageEngine::new(Arc::clone(&metrics)));
        engine
            .open_drives(
                Some(dir.path()),
                &[],
                SyncMode::Fdatasync,
                Duration::from_millis(2),
                1024 * 1024,
                "n1",
            )
            .await
            .unwrap();
        let wal = engine.get("default").await.unwrap();
        let cid = Uuid::new_v4();
        let key = [0x42u8; 32];
        let payload = kv_payload(b"k", b"secret");
        wal.append_durable(cid, 1, WalRecordType::Put, payload, Some(&key))
            .await
            .unwrap();

        // Ciphertext must not match plaintext.
        let recs = wal.replay_all().await.unwrap();
        assert_eq!(recs.len(), 1);
        assert_ne!(&recs[0].payload, &kv_payload(b"k", b"secret"));

        let catalog = vec![CatalogEntry {
            container_id: cid,
            mode: StorageMode::Persistent,
            key_ref: Some("dk1".into()),
        }];
        let mut keys = WalDataKeys::new();
        keys.insert(cid, Some(key));
        restore_before_ready(&engine, &catalog, &HashMap::new(), &metrics, &keys)
            .await
            .unwrap();
        let content = engine.content.read().await;
        assert_eq!(content.get(cid, b"k"), Some(b"secret".as_slice()));
        assert!(!content.is_unavailable(cid));
    }

    #[tokio::test]
    async fn missing_key_marks_unavailable_not_ciphertext() {
        let dir = tempdir().unwrap();
        let metrics = Arc::new(WalMetrics::default());
        let engine = Arc::new(StorageEngine::new(Arc::clone(&metrics)));
        engine
            .open_drives(
                Some(dir.path()),
                &[],
                SyncMode::Fdatasync,
                Duration::from_millis(2),
                1024 * 1024,
                "n1",
            )
            .await
            .unwrap();
        let wal = engine.get("default").await.unwrap();
        let cid = Uuid::new_v4();
        let key = [0x7u8; 32];
        let payload = kv_payload(b"k", b"secret");
        wal.append_durable(cid, 1, WalRecordType::Put, payload, Some(&key))
            .await
            .unwrap();

        let catalog = vec![CatalogEntry {
            container_id: cid,
            mode: StorageMode::Persistent,
            key_ref: Some("missing".into()),
        }];
        // Encrypted + unresolved key.
        let mut keys = WalDataKeys::new();
        keys.insert(cid, None);
        let report =
            restore_before_ready(&engine, &catalog, &HashMap::new(), &metrics, &keys)
                .await
                .unwrap();
        assert_eq!(report.applied, 0);
        assert!(report.skipped_unavailable >= 1);
        let content = engine.content.read().await;
        assert!(content.is_unavailable(cid));
        assert_eq!(
            content.unavailable_reason(cid),
            Some(&UnavailableReason::KeyUnresolvable)
        );
        assert_eq!(content.get(cid, b"k"), None);
        assert_eq!(content.len(cid), 0);
    }
}
