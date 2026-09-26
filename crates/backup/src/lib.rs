//! Snapshot / PITR / restore-fill library (013 US4; operator jobs via 010).

pub mod manifest;
pub mod pitr;
pub mod restore;
pub mod snapshot;

pub use manifest::{
    ContainerSnapshotMeta, SnapshotManifest, SnapshotScope, SnapshotScopeKind, FORMAT_MAJOR,
    SNAPSHOT_MAGIC,
};
pub use pitr::{map_hlc_to_positions, resolve_positions, PitrError, PitrTarget, StampLsn};
pub use restore::{RestoreRequest, RestoreService};
pub use snapshot::{
    create_snapshot, load_manifest, scope_container, scope_namespace, SnapshotCreateRequest,
    SnapshotService,
};

use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum BackupError {
    #[error("backup / PITR operator jobs require slice 10")]
    BackupSlice10Required,
    #[error("restore destination has content: {container}")]
    RestoreDestinationHasContent { container: String },
    #[error("key ref missing: {key_ref}")]
    KeyRefMissing { key_ref: String },
    #[error("pitr_unmappable: drive {drive_id}")]
    PitrUnmappable { drive_id: String },
    #[error("io: {0}")]
    Io(String),
}

impl From<PitrError> for BackupError {
    fn from(e: PitrError) -> Self {
        match e {
            PitrError::Unmappable { drive_id } => Self::PitrUnmappable { drive_id },
            PitrError::PastDurable { drive_id, .. } => Self::PitrUnmappable { drive_id },
        }
    }
}

impl BackupError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::BackupSlice10Required => "BackupSlice10Required",
            Self::RestoreDestinationHasContent { .. } => "RestoreDestinationHasContent",
            Self::KeyRefMissing { .. } => "KeyRefMissing",
            Self::PitrUnmappable { .. } => "pitr_unmappable",
            Self::Io(_) => "io",
        }
    }
}
