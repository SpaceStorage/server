//! Durable Raft log + snapshot layout. Fsync ONLY via `spawn_blocking` (constitution II).
//!
//! ## Raft implementation choice
//!
//! Prefer `openraft` per research R2. Workspace edition 2024 + MSRV 1.85 currently
//! fights stable openraft 0.9 / alpha 0.10 trait churn for a clean pin. This module
//! (with [`crate::raft_net`] and the in-process group runtime) implements a **correct
//! lightweight Raft**: RequestVote / AppendEntries / majority commit / odd voter sets /
//! snapshot install — same contracts as [contracts/raft-rpc.md](../../../../specs/006-control-plane/contracts/raft-rpc.md).
//! Revisit openraft when a stable edition-2024 pin lands.

use crate::error::{ControlPlaneError, Result};
use crate::group::GroupId;
use crate::membership::MembershipOp;
use serde::{Deserialize, Serialize};
use spacestorage_clocks::HlcStamp;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// On-disk format version. Unknown → refuse start (`UnknownRaftFormat`).
pub const RAFT_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RaftBody {
    Noop,
    Cluster(serde_json::Value),
    Namespace(serde_json::Value),
    Membership(MembershipOp),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RaftLogRecord {
    pub group: GroupId,
    pub term: u64,
    pub index: u64,
    pub hlc: HlcStamp,
    pub body: RaftBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SegmentHeader {
    format_version: u32,
    group: GroupId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMeta {
    pub format_version: u32,
    pub group: GroupId,
    pub last_included_index: u64,
    pub last_included_term: u64,
    pub data: serde_json::Value,
}

/// Persistent log storage for one Raft group.
pub struct RaftStore {
    pub group: GroupId,
    dir: PathBuf,
    records: Vec<RaftLogRecord>,
    pub snapshot: Option<SnapshotMeta>,
    pub current_term: u64,
    pub voted_for: Option<Uuid>,
    pub commit_index: u64,
}

impl RaftStore {
    pub async fn open(data_dir: &Path, group: GroupId) -> Result<Self> {
        let dir = group.raft_dir(data_dir);
        let dir_clone = dir.clone();
        tokio::task::spawn_blocking(move || {
            std::fs::create_dir_all(&dir_clone).map_err(|e| ControlPlaneError::Io(e.to_string()))
        })
        .await
        .map_err(|e| ControlPlaneError::Io(e.to_string()))??;

        let mut store = Self {
            group,
            dir,
            records: Vec::new(),
            snapshot: None,
            current_term: 0,
            voted_for: None,
            commit_index: 0,
        };
        store.load().await?;
        Ok(store)
    }

    async fn load(&mut self) -> Result<()> {
        let meta_path = self.dir.join("meta.json");
        let log_path = self.dir.join("log.jsonl");
        let snap_path = self.dir.join("snapshot.json");
        let meta_c = meta_path.clone();
        let log_c = log_path.clone();
        let snap_c = snap_path.clone();
        let loaded = tokio::task::spawn_blocking(move || -> Result<(u64, Option<Uuid>, u64, Vec<RaftLogRecord>, Option<SnapshotMeta>)> {
            let (term, voted, commit) = if meta_c.exists() {
                let bytes = std::fs::read(&meta_c).map_err(|e| ControlPlaneError::Io(e.to_string()))?;
                let v: serde_json::Value = serde_json::from_slice(&bytes)
                    .map_err(|e| ControlPlaneError::Io(e.to_string()))?;
                let ver = v.get("format_version").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
                if ver != RAFT_FORMAT_VERSION {
                    return Err(ControlPlaneError::UnknownRaftFormat { version: ver });
                }
                (
                    v.get("current_term").and_then(|x| x.as_u64()).unwrap_or(0),
                    v.get("voted_for")
                        .and_then(|x| x.as_str())
                        .and_then(|s| Uuid::parse_str(s).ok()),
                    v.get("commit_index").and_then(|x| x.as_u64()).unwrap_or(0),
                )
            } else {
                (0, None, 0)
            };
            let mut records = Vec::new();
            if log_c.exists() {
                let text = std::fs::read_to_string(&log_c)
                    .map_err(|e| ControlPlaneError::Io(e.to_string()))?;
                for (i, line) in text.lines().enumerate() {
                    if line.trim().is_empty() {
                        continue;
                    }
                    let rec: RaftLogRecord = serde_json::from_str(line).map_err(|e| {
                        ControlPlaneError::Io(format!("log line {i}: {e}"))
                    })?;
                    records.push(rec);
                }
            }
            let snapshot = if snap_c.exists() {
                let bytes = std::fs::read(&snap_c).map_err(|e| ControlPlaneError::Io(e.to_string()))?;
                let snap: SnapshotMeta = serde_json::from_slice(&bytes)
                    .map_err(|e| ControlPlaneError::Io(e.to_string()))?;
                if snap.format_version != RAFT_FORMAT_VERSION {
                    return Err(ControlPlaneError::UnknownRaftFormat {
                        version: snap.format_version,
                    });
                }
                Some(snap)
            } else {
                None
            };
            Ok((term, voted, commit, records, snapshot))
        })
        .await
        .map_err(|e| ControlPlaneError::Io(e.to_string()))??;

        self.current_term = loaded.0;
        self.voted_for = loaded.1;
        self.commit_index = loaded.2;
        self.records = loaded.3;
        self.snapshot = loaded.4;
        Ok(())
    }

    pub async fn persist_meta(&self) -> Result<()> {
        let path = self.dir.join("meta.json");
        let header = SegmentHeader {
            format_version: RAFT_FORMAT_VERSION,
            group: self.group,
        };
        let body = serde_json::json!({
            "format_version": header.format_version,
            "group": header.group,
            "current_term": self.current_term,
            "voted_for": self.voted_for.map(|u| u.to_string()),
            "commit_index": self.commit_index,
        });
        let bytes = serde_json::to_vec_pretty(&body)
            .map_err(|e| ControlPlaneError::Io(e.to_string()))?;
        tokio::task::spawn_blocking(move || {
            atomic_write_fsync(&path, &bytes)
        })
        .await
        .map_err(|e| ControlPlaneError::Io(e.to_string()))?
    }

    pub async fn append_records(&mut self, entries: &[RaftLogRecord]) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        self.records.extend(entries.iter().cloned());
        let path = self.dir.join("log.jsonl");
        let mut buf = String::new();
        for r in entries {
            buf.push_str(
                &serde_json::to_string(r).map_err(|e| ControlPlaneError::Io(e.to_string()))?,
            );
            buf.push('\n');
        }
        let bytes = buf.into_bytes();
        tokio::task::spawn_blocking(move || append_fsync(&path, &bytes))
            .await
            .map_err(|e| ControlPlaneError::Io(e.to_string()))??;
        self.persist_meta().await
    }

    pub fn last_index(&self) -> u64 {
        self.records.last().map(|r| r.index).unwrap_or_else(|| {
            self.snapshot
                .as_ref()
                .map(|s| s.last_included_index)
                .unwrap_or(0)
        })
    }

    pub fn last_term(&self) -> u64 {
        self.records.last().map(|r| r.term).unwrap_or_else(|| {
            self.snapshot
                .as_ref()
                .map(|s| s.last_included_term)
                .unwrap_or(0)
        })
    }

    pub fn entries_from(&self, from_index: u64) -> Vec<RaftLogRecord> {
        self.records
            .iter()
            .filter(|r| r.index >= from_index)
            .cloned()
            .collect()
    }

    pub fn truncate_from(&mut self, index: u64) {
        self.records.retain(|r| r.index < index);
    }

    pub async fn install_snapshot(&mut self, snap: SnapshotMeta) -> Result<()> {
        if snap.format_version != RAFT_FORMAT_VERSION {
            return Err(ControlPlaneError::UnknownRaftFormat {
                version: snap.format_version,
            });
        }
        let path = self.dir.join("snapshot.json");
        let bytes = serde_json::to_vec_pretty(&snap)
            .map_err(|e| ControlPlaneError::Io(e.to_string()))?;
        tokio::task::spawn_blocking(move || atomic_write_fsync(&path, &bytes))
            .await
            .map_err(|e| ControlPlaneError::Io(e.to_string()))??;
        self.records
            .retain(|r| r.index > snap.last_included_index);
        self.snapshot = Some(snap);
        self.persist_meta().await
    }

    /// Quarantine this group's directory (corrupt / unknown format recovery).
    pub async fn quarantine(data_dir: &Path, group: GroupId) -> Result<PathBuf> {
        let dir = group.raft_dir(data_dir);
        spacestorage_storage::quarantine::quarantine(&dir)
            .await
            .map_err(|e| ControlPlaneError::Io(e.to_string()))
    }
}

fn atomic_write_fsync(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| ControlPlaneError::Io(e.to_string()))?;
    let f = std::fs::File::open(&tmp).map_err(|e| ControlPlaneError::Io(e.to_string()))?;
    f.sync_all().map_err(|e| ControlPlaneError::Io(e.to_string()))?;
    drop(f);
    std::fs::rename(&tmp, path).map_err(|e| ControlPlaneError::Io(e.to_string()))?;
    if let Some(parent) = path.parent() {
        let dir = std::fs::File::open(parent).map_err(|e| ControlPlaneError::Io(e.to_string()))?;
        let _ = dir.sync_all();
    }
    Ok(())
}

fn append_fsync(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| ControlPlaneError::Io(e.to_string()))?;
    f.write_all(bytes)
        .map_err(|e| ControlPlaneError::Io(e.to_string()))?;
    f.sync_all()
        .map_err(|e| ControlPlaneError::Io(e.to_string()))?;
    Ok(())
}
