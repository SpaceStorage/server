//! Per-drive WAL: `{drive.path}/wal/<seq>.wal`.

mod group_commit;
mod record;
mod replay;

pub use group_commit::GroupCommitConfig;
pub use record::{
    encode_record, encode_segment_header, maybe_decrypt_payload, maybe_encrypt_payload,
    parse_records, wal_payload_aad, WalRecord, WalRecordType, FORMAT_MAJOR, WAL_MAGIC,
};
pub use replay::replay_segment;

use crate::error::StorageError;
use crate::metrics::WalMetrics;
use crate::quarantine;
use crate::AckKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    Fdatasync,
    Fsync,
    /// Only under SPACESTORAGE_TEST=1; never counts as durable ack.
    None,
}

impl SyncMode {
    pub fn parse(s: &str) -> Result<Self, StorageError> {
        match s.to_ascii_lowercase().as_str() {
            "fdatasync" | "" => Ok(Self::Fdatasync),
            "fsync" => Ok(Self::Fsync),
            "none" => {
                let test = std::env::var("SPACESTORAGE_TEST").ok().as_deref() == Some("1");
                if test {
                    Ok(Self::None)
                } else {
                    Err(StorageError::SyncNoneNotDurable)
                }
            }
            _ => Ok(Self::Fdatasync),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DurableAck {
    pub replica: String,
    pub drive_id: String,
    pub lsn: u64,
    pub kind: AckKind,
}

struct Pending {
    lsn: u64,
    bytes: Vec<u8>,
    notify: Arc<Notify>,
    done: Arc<Mutex<Option<Result<(), StorageError>>>>,
}

pub struct DriveWal {
    pub drive_id: String,
    pub path: PathBuf,
    pub format_major: u16,
    pub sync: SyncMode,
    next_lsn: AtomicU64,
    durable_lsn: AtomicU64,
    current_seq: AtomicU64,
    segment_max: u64,
    group_max_wait: Duration,
    group_max_bytes: usize,
    node_id: String,
    metrics: Arc<WalMetrics>,
    pending: Mutex<Vec<Pending>>,
    flusher: Mutex<()>,
    quarantined: StdMutex<Vec<String>>,
}

impl DriveWal {
    pub async fn open(
        drive_id: String,
        drive_path: PathBuf,
        sync: SyncMode,
        group_max_wait: Duration,
        group_max_bytes: usize,
        node_id: &str,
        metrics: Arc<WalMetrics>,
    ) -> Result<Self, StorageError> {
        let wal_dir = drive_path.join("wal");
        let wal_dir_clone = wal_dir.clone();
        tokio::task::spawn_blocking(move || std::fs::create_dir_all(&wal_dir_clone))
            .await
            .map_err(|e| StorageError::Join(e.to_string()))??;

        let (next_lsn, durable_lsn, current_seq, quarantined) =
            Self::scan_existing(&wal_dir, &drive_id, metrics.as_ref()).await?;

        let wal = Self {
            drive_id,
            path: wal_dir,
            format_major: FORMAT_MAJOR,
            sync,
            next_lsn: AtomicU64::new(next_lsn),
            durable_lsn: AtomicU64::new(durable_lsn),
            current_seq: AtomicU64::new(current_seq),
            segment_max: 64 * 1024 * 1024,
            group_max_wait,
            group_max_bytes,
            node_id: node_id.to_string(),
            metrics,
            pending: Mutex::new(Vec::new()),
            flusher: Mutex::new(()),
            quarantined: StdMutex::new(quarantined),
        };
        wal.ensure_segment().await?;
        Ok(wal)
    }

    async fn scan_existing(
        wal_dir: &Path,
        drive_id: &str,
        metrics: &WalMetrics,
    ) -> Result<(u64, u64, u64, Vec<String>), StorageError> {
        let dir = wal_dir.to_path_buf();
        let drive_id = drive_id.to_string();
        let result = tokio::task::spawn_blocking(
            move || -> Result<(u64, u64, u64, u64, Vec<String>), StorageError> {
                let mut max_lsn = 0u64;
                let mut max_seq = 0u64;
                let mut torn = 0u64;
                let mut quarantined = Vec::new();
                if !dir.exists() {
                    return Ok((1, 0, 0, 0, quarantined));
                }
                let mut entries: Vec<_> = std::fs::read_dir(&dir)?
                    .filter_map(|e| e.ok())
                    .filter(|e| {
                        e.path()
                            .extension()
                            .and_then(|x| x.to_str())
                            .is_some_and(|x| x == "wal")
                    })
                    .collect();
                entries.sort_by_key(|e| e.file_name());
                for e in entries {
                    let path = e.path();
                    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("0");
                    if let Ok(seq) = name.parse::<u64>() {
                        max_seq = max_seq.max(seq);
                    }
                    let data = std::fs::read(&path)?;
                    match replay::replay_bytes(&data, &drive_id) {
                        Ok((recs, t)) => {
                            torn += t;
                            for r in recs {
                                max_lsn = max_lsn.max(r.lsn);
                            }
                        }
                        Err(StorageError::WalFormatUnsupported { .. })
                        | Err(StorageError::FormatUnsupported { .. }) => {
                            return Err(StorageError::FormatUnsupported {
                                path: path.display().to_string(),
                            });
                        }
                        Err(StorageError::WalCorrupt(_)) => {
                            let dest = quarantine::quarantine_sync(&path)?;
                            quarantined.push(dest.display().to_string());
                        }
                        Err(e) => return Err(e),
                    }
                }
                Ok((
                    max_lsn.saturating_add(1).max(1),
                    max_lsn,
                    max_seq,
                    torn,
                    quarantined,
                ))
            },
        )
        .await
        .map_err(|e| StorageError::Join(e.to_string()))??;
        metrics
            .wal_torn_total
            .fetch_add(result.3, Ordering::Relaxed);
        Ok((result.0, result.1, result.2, result.4))
    }

    pub fn take_quarantined(&self) -> Vec<String> {
        std::mem::take(&mut *self.quarantined.lock().unwrap())
    }

    fn segment_path(&self, seq: u64) -> PathBuf {
        self.path.join(format!("{seq:016}.wal"))
    }

    async fn ensure_segment(&self) -> Result<(), StorageError> {
        let seq = self.current_seq.load(Ordering::SeqCst);
        let path = self.segment_path(seq);
        if path.exists() {
            return Ok(());
        }
        let header = encode_segment_header(FORMAT_MAJOR, &self.drive_id, &self.node_id);
        let p = path.clone();
        tokio::task::spawn_blocking(move || std::fs::write(&p, header))
            .await
            .map_err(|e| StorageError::Join(e.to_string()))??;
        Ok(())
    }

    pub fn next_lsn(&self) -> u64 {
        self.next_lsn.load(Ordering::SeqCst)
    }

    pub fn durable_lsn(&self) -> u64 {
        self.durable_lsn.load(Ordering::SeqCst)
    }

    /// Append a record and wait until durable (or memory ack for sync none).
    /// When `data_key` is `Some`, encrypt payload with AAD `drive_id ‖ lsn ‖ container_id`.
    pub async fn append_durable(
        &self,
        container_id: Uuid,
        stamp: u64,
        ty: WalRecordType,
        payload: Vec<u8>,
        data_key: Option<&[u8; 32]>,
    ) -> Result<DurableAck, StorageError> {
        let lsn = self.next_lsn.fetch_add(1, Ordering::SeqCst);
        let aad = wal_payload_aad(&self.drive_id, lsn, container_id);
        let payload = maybe_encrypt_payload(&payload, data_key, &aad)?;
        let rec = WalRecord {
            lsn,
            container_id,
            stamp,
            ty,
            payload,
        };
        let bytes = encode_record(&rec);
        let notify = Arc::new(Notify::new());
        let done = Arc::new(Mutex::new(None));
        {
            let mut q = self.pending.lock().await;
            q.push(Pending {
                lsn,
                bytes,
                notify: Arc::clone(&notify),
                done: Arc::clone(&done),
            });
        }
        self.flush_group().await?;
        loop {
            if let Some(result) = done.lock().await.take() {
                result?;
                break;
            }
            notify.notified().await;
        }
        let kind = if matches!(self.sync, SyncMode::None) {
            AckKind::Memory
        } else {
            AckKind::Durable
        };
        Ok(DurableAck {
            replica: self.node_id.clone(),
            drive_id: self.drive_id.clone(),
            lsn,
            kind,
        })
    }

    async fn flush_group(&self) -> Result<(), StorageError> {
        let _guard = self.flusher.lock().await;
        let deadline = tokio::time::Instant::now() + self.group_max_wait;
        loop {
            let total: usize = {
                let q = self.pending.lock().await;
                q.iter().map(|p| p.bytes.len()).sum()
            };
            if total == 0 {
                return Ok(());
            }
            if total >= self.group_max_bytes || tokio::time::Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(Duration::from_micros(200)).await;
        }

        let batch: Vec<Pending> = {
            let mut q = self.pending.lock().await;
            std::mem::take(&mut *q)
        };
        if batch.is_empty() {
            return Ok(());
        }

        let seq = self.current_seq.load(Ordering::SeqCst);
        let path = self.segment_path(seq);
        let sync = self.sync;
        let mut blob = Vec::new();
        let mut max_lsn = 0u64;
        for p in &batch {
            blob.extend_from_slice(&p.bytes);
            max_lsn = max_lsn.max(p.lsn);
        }
        let bytes_len = blob.len() as u64;
        let path_clone = path.clone();

        let start = std::time::Instant::now();
        let write_result =
            tokio::task::spawn_blocking(move || -> Result<(), StorageError> {
                use std::io::Write;
                let mut f = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path_clone)?;
                f.write_all(&blob)?;
                match sync {
                    SyncMode::None => {}
                    // Constitution II: durability syscall runs only on blocking pool.
                    SyncMode::Fdatasync => f.sync_data()?,
                    SyncMode::Fsync => f.sync_all()?,
                }
                Ok(())
            })
            .await
            .map_err(|e| StorageError::Join(e.to_string()))?;

        let dur = start.elapsed();
        match write_result {
            Ok(()) => {
                if !matches!(sync, SyncMode::None) {
                    self.durable_lsn.store(max_lsn, Ordering::SeqCst);
                    self.metrics.record_fsync(bytes_len, dur);
                }
                let meta_path = path.clone();
                let size = tokio::task::spawn_blocking(move || {
                    std::fs::metadata(&meta_path).map(|m| m.len()).unwrap_or(0)
                })
                .await
                .unwrap_or(0);
                if size >= self.segment_max {
                    self.current_seq.fetch_add(1, Ordering::SeqCst);
                    self.ensure_segment().await?;
                }
                for p in batch {
                    *p.done.lock().await = Some(Ok(()));
                    p.notify.notify_one();
                }
                Ok(())
            }
            Err(e) => {
                for p in &batch {
                    *p.done.lock().await = Some(Err(StorageError::DiskFull {
                        drive_id: self.drive_id.clone(),
                    }));
                    p.notify.notify_one();
                }
                Err(e)
            }
        }
    }

    pub async fn replay_all(&self) -> Result<Vec<WalRecord>, StorageError> {
        let dir = self.path.clone();
        let drive_id = self.drive_id.clone();
        let start = std::time::Instant::now();
        let (recs, torn, quarantined) = tokio::task::spawn_blocking(move || {
            let mut all = Vec::new();
            let mut torn = 0u64;
            let mut quarantined = Vec::new();
            if !dir.exists() {
                return Ok::<_, StorageError>((all, torn, quarantined));
            }
            let mut entries: Vec<_> = std::fs::read_dir(&dir)?
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .and_then(|x| x.to_str())
                        .is_some_and(|x| x == "wal")
                })
                .collect();
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                let path = e.path();
                let data = std::fs::read(&path)?;
                match replay::replay_bytes(&data, &drive_id) {
                    Ok((mut recs, t)) => {
                        torn += t;
                        all.append(&mut recs);
                    }
                    Err(StorageError::WalCorrupt(_)) => {
                        let dest = quarantine::quarantine_sync(&path)?;
                        quarantined.push(dest.display().to_string());
                    }
                    Err(e) => return Err(e),
                }
            }
            Ok((all, torn, quarantined))
        })
        .await
        .map_err(|e| StorageError::Join(e.to_string()))??;
        if !quarantined.is_empty() {
            let mut q = self.quarantined.lock().unwrap();
            q.extend(quarantined);
        }
        self.metrics
            .wal_torn_total
            .fetch_add(torn, Ordering::Relaxed);
        self.metrics.wal_replay_duration_ns.fetch_add(
            start.elapsed().as_nanos() as u64,
            Ordering::Relaxed,
        );
        self.metrics
            .recovery_records_total
            .fetch_add(recs.len() as u64, Ordering::Relaxed);
        Ok(recs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn durable_append_and_replay() {
        let dir = tempdir().unwrap();
        let metrics = Arc::new(WalMetrics::default());
        let wal = DriveWal::open(
            "default".into(),
            dir.path().to_path_buf(),
            SyncMode::Fdatasync,
            Duration::from_millis(2),
            1024 * 1024,
            "n1",
            Arc::clone(&metrics),
        )
        .await
        .unwrap();
        let ack = wal
            .append_durable(Uuid::new_v4(), 1, WalRecordType::Put, b"hello".to_vec(), None)
            .await
            .unwrap();
        assert_eq!(ack.kind, AckKind::Durable);
        assert!(wal.durable_lsn() >= ack.lsn);
        let recs = wal.replay_all().await.unwrap();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].payload, b"hello");
        assert!(metrics.wal_fsync_total.load(Ordering::Relaxed) >= 1);
    }

    #[tokio::test]
    async fn encrypt_payload_when_key_provided() {
        let dir = tempdir().unwrap();
        let metrics = Arc::new(WalMetrics::default());
        let wal = DriveWal::open(
            "default".into(),
            dir.path().to_path_buf(),
            SyncMode::Fdatasync,
            Duration::from_millis(2),
            1024 * 1024,
            "n1",
            Arc::clone(&metrics),
        )
        .await
        .unwrap();
        let key = [7u8; 32];
        let ack = wal
            .append_durable(
                Uuid::nil(),
                1,
                WalRecordType::Put,
                b"secret".to_vec(),
                Some(&key),
            )
            .await
            .unwrap();
        let recs = wal.replay_all().await.unwrap();
        assert_eq!(recs.len(), 1);
        assert_ne!(recs[0].payload, b"secret");
        assert!(recs[0].payload.len() > 12);
        assert!(wal.durable_lsn() >= ack.lsn);
        let aad = wal_payload_aad("default", recs[0].lsn, Uuid::nil());
        let pt = maybe_decrypt_payload(&recs[0].payload, Some(&key), &aad).unwrap();
        assert_eq!(pt, b"secret");
    }

    #[tokio::test]
    async fn mid_log_corrupt_quarantines_segment() {
        let dir = tempdir().unwrap();
        let wal_dir = dir.path().join("wal");
        std::fs::create_dir_all(&wal_dir).unwrap();
        let mut data = encode_segment_header(FORMAT_MAJOR, "default", "n1");
        let good = WalRecord {
            lsn: 1,
            container_id: Uuid::nil(),
            stamp: 0,
            ty: WalRecordType::Put,
            payload: b"ok".to_vec(),
        };
        data.extend_from_slice(&encode_record(&good));
        // Mid-log CRC-invalid frame followed by another valid-looking byte so it is not a torn tail.
        let mut bad = encode_record(&WalRecord {
            lsn: 2,
            container_id: Uuid::nil(),
            stamp: 0,
            ty: WalRecordType::Put,
            payload: b"xx".to_vec(),
        });
        // Flip CRC bytes.
        bad[4] ^= 0xff;
        data.extend_from_slice(&bad);
        data.extend_from_slice(&encode_record(&WalRecord {
            lsn: 3,
            container_id: Uuid::nil(),
            stamp: 0,
            ty: WalRecordType::Put,
            payload: b"tail".to_vec(),
        }));
        let seg = wal_dir.join("0000000000000000.wal");
        std::fs::write(&seg, &data).unwrap();

        let metrics = Arc::new(WalMetrics::default());
        let wal = DriveWal::open(
            "default".into(),
            dir.path().to_path_buf(),
            SyncMode::Fdatasync,
            Duration::from_millis(2),
            1024 * 1024,
            "n1",
            metrics,
        )
        .await
        .unwrap();
        let q = wal.take_quarantined();
        assert!(!q.is_empty(), "expected quarantine rename");
        assert!(
            q.iter().any(|p| p.contains(".corrupt.")),
            "quarantine dest missing .corrupt. suffix: {q:?}"
        );
        // ensure_segment may recreate an empty segment at the same seq after quarantine.
        assert!(std::path::Path::new(&q[0]).exists());
    }
}
