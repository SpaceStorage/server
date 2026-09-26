//! Counted write acknowledgements (013 US1 / T062).
//!
//! Persistent/hybrid counted acks are crash-durable only when `kind == Durable`
//! and the replica’s pinned drive has `durable_lsn ≥ record.lsn`. Memory-mode
//! acks (and `sync none` memory-kind WAL acks) MUST NOT be labelled crash-durable.

use crate::definition::StorageModeChoice;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AckKind {
    Durable,
    Memory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteAck {
    pub replica: String,
    pub drive_id: String,
    pub lsn: u64,
    pub kind: AckKind,
    /// Container storage mode that produced this ack.
    pub mode: StorageModeChoice,
}

impl WriteAck {
    /// True when this ack may satisfy persistent/hybrid quorum.
    pub fn counts_as_crash_durable(&self) -> bool {
        matches!(self.kind, AckKind::Durable)
            && matches!(
                self.mode,
                StorageModeChoice::Persistent | StorageModeChoice::Hybrid
            )
    }

    /// Build a labelled ack: memory-mode or memory-kind WAL never claim durable.
    pub fn from_wal(
        replica: impl Into<String>,
        drive_id: impl Into<String>,
        lsn: u64,
        wal_kind: AckKind,
        mode: StorageModeChoice,
        durable_lsn: u64,
    ) -> Self {
        let kind = match mode {
            StorageModeChoice::Memory => AckKind::Memory,
            StorageModeChoice::Persistent | StorageModeChoice::Hybrid => {
                if matches!(wal_kind, AckKind::Durable) && durable_lsn >= lsn {
                    AckKind::Durable
                } else {
                    AckKind::Memory
                }
            }
        };
        Self {
            replica: replica.into(),
            drive_id: drive_id.into(),
            lsn,
            kind,
            mode,
        }
    }
}
