//! Snapshot manifest (`SNP1` major 1) — 013 US4 / data-model §6.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

pub const SNAPSHOT_MAGIC: &str = "SNP1";
pub const FORMAT_MAJOR: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotScopeKind {
    Container,
    Namespace,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotScope {
    pub kind: SnapshotScopeKind,
    /// Namespace name, or `namespace/container` for container scope.
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerSnapshotMeta {
    pub id: Uuid,
    pub namespace: String,
    pub name: String,
    pub type_name: String,
    /// `memory` | `persistent` | `hybrid`
    pub mode: String,
    /// Key reference name when encrypted; omitted/null when plaintext.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotManifest {
    pub magic: String,
    pub format_major: u16,
    pub snapshot_id: Uuid,
    pub scope: SnapshotScope,
    /// Independent per-drive cut positions (not a single cluster LSN).
    pub positions: BTreeMap<String, u64>,
    pub containers: Vec<ContainerSnapshotMeta>,
    pub key_refs: Vec<String>,
}

impl SnapshotManifest {
    pub fn new(
        snapshot_id: Uuid,
        scope: SnapshotScope,
        positions: BTreeMap<String, u64>,
        containers: Vec<ContainerSnapshotMeta>,
    ) -> Self {
        let mut key_refs: Vec<String> = containers
            .iter()
            .filter_map(|c| c.key_ref.clone())
            .collect();
        key_refs.sort();
        key_refs.dedup();
        Self {
            magic: SNAPSHOT_MAGIC.into(),
            format_major: FORMAT_MAJOR,
            snapshot_id,
            scope,
            positions,
            containers,
            key_refs,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.magic != SNAPSHOT_MAGIC {
            return Err(format!("bad magic {}", self.magic));
        }
        if self.format_major != FORMAT_MAJOR {
            return Err(format!("unsupported format_major {}", self.format_major));
        }
        Ok(())
    }
}
