//! Replication accept path: durable ACK only after `SourceLog::append_durable` (T071).

use crate::source_log::{SourceLog, SourceLogError, SourceLogPosition};
use serde_json::json;
use std::path::PathBuf;
use tokio::sync::Mutex;

/// Lazy source-log accept seam for the replication handler.
pub struct DurableAccept {
    dir: PathBuf,
    epoch: u64,
    log: Mutex<Option<SourceLog>>,
}

impl DurableAccept {
    pub fn new(dir: PathBuf, epoch: u64) -> Self {
        Self {
            dir,
            epoch,
            log: Mutex::new(None),
        }
    }

    /// Append payload via `spawn_blocking` fsync; ACK JSON has `durable: true` only on success.
    pub async fn ack_after_durable(&self, payload: Vec<u8>) -> Result<Vec<u8>, SourceLogError> {
        let mut guard = self.log.lock().await;
        if guard.is_none() {
            *guard = Some(SourceLog::open(&self.dir, self.epoch).await?);
        }
        let pos = guard
            .as_mut()
            .expect("opened above")
            .append_durable(payload)
            .await?;
        Ok(ack_durable_json(&pos))
    }
}

pub fn ack_durable_json(pos: &SourceLogPosition) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "ok": true,
        "durable": true,
        "epoch": pos.epoch,
        "position": pos.position,
    }))
    .expect("ack json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn batch_ack_is_durable_after_fsync() {
        let dir = tempdir().unwrap();
        let accept = DurableAccept::new(dir.path().to_path_buf(), 1);
        let ack = accept.ack_after_durable(b"batch".to_vec()).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&ack).unwrap();
        assert_eq!(v["durable"], true);
        assert_eq!(v["ok"], true);
        assert!(dir.path().join("source_log.jsonl").exists());
    }
}
