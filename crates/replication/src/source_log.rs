//! Durable source log — fsync via `spawn_blocking` (Constitution II / T069).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SourceLogError {
    #[error("io: {0}")]
    Io(String),
    #[error("epoch_fenced")]
    EpochFenced,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceLogPosition {
    pub epoch: u64,
    pub position: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LogRecord {
    position: u64,
    epoch: u64,
    payload: Vec<u8>,
}

/// Append-only source log under `{dir}/source_log.jsonl` with durable ack after fsync.
pub struct SourceLog {
    path: PathBuf,
    epoch: u64,
    next_pos: u64,
}

impl SourceLog {
    pub async fn open(dir: &Path, epoch: u64) -> Result<Self, SourceLogError> {
        tokio::fs::create_dir_all(dir)
            .await
            .map_err(|e| SourceLogError::Io(e.to_string()))?;
        let path = dir.join("source_log.jsonl");
        let next_pos = if path.exists() {
            let text = tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| SourceLogError::Io(e.to_string()))?;
            text.lines()
                .filter_map(|l| serde_json::from_str::<LogRecord>(l).ok())
                .map(|r| r.position)
                .max()
                .map(|p| p + 1)
                .unwrap_or(0)
        } else {
            0
        };
        Ok(Self {
            path,
            epoch,
            next_pos,
        })
    }

    pub fn position(&self) -> SourceLogPosition {
        SourceLogPosition {
            epoch: self.epoch,
            position: self.next_pos.saturating_sub(1),
        }
    }

    /// Append + durable fsync on blocking pool; returns position after durability.
    pub async fn append_durable(&mut self, payload: Vec<u8>) -> Result<SourceLogPosition, SourceLogError> {
        let pos = self.next_pos;
        let rec = LogRecord {
            position: pos,
            epoch: self.epoch,
            payload,
        };
        let line = serde_json::to_string(&rec).map_err(|e| SourceLogError::Io(e.to_string()))?;
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|e| SourceLogError::Io(e.to_string()))?;
            writeln!(f, "{line}").map_err(|e| SourceLogError::Io(e.to_string()))?;
            f.sync_all().map_err(|e| SourceLogError::Io(e.to_string()))?;
            Ok::<(), SourceLogError>(())
        })
        .await
        .map_err(|e| SourceLogError::Io(e.to_string()))??;
        self.next_pos = pos + 1;
        Ok(SourceLogPosition {
            epoch: self.epoch,
            position: pos,
        })
    }

    pub fn fence_epoch(&mut self, new_epoch: u64) -> Result<(), SourceLogError> {
        if new_epoch <= self.epoch {
            return Err(SourceLogError::EpochFenced);
        }
        self.epoch = new_epoch;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn append_fsyncs_via_spawn_blocking() {
        let dir = tempdir().unwrap();
        let mut log = SourceLog::open(dir.path(), 1).await.unwrap();
        let p = log.append_durable(b"hello".to_vec()).await.unwrap();
        assert_eq!(p.position, 0);
        assert!(dir.path().join("source_log.jsonl").exists());
        let log2 = SourceLog::open(dir.path(), 1).await.unwrap();
        assert_eq!(log2.next_pos, 1);
    }
}
