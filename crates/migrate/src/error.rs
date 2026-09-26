//! Named migration / transform / backup job errors (010 contracts/jobs.md).

use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MigrateError {
    #[error("data migration / transform / backup jobs require slice 10")]
    MigrateSlice10Required,
    #[error("name exists: {container}")]
    NameExists { container: String },
    #[error("quota exceeded")]
    QuotaExceeded,
    #[error("placement constraint unsatisfiable")]
    ConstraintUnsatisfiable,
    #[error("mapping query required for complex transform")]
    MappingQueryRequired,
    #[error("mapping source missing: {name}")]
    MappingSourceMissing { name: String },
    #[error("job already in progress on source")]
    JobInProgress,
    #[error("authz denied")]
    AuthzDenied,
    #[error("recorded job state unreadable")]
    StateUnreadable,
    #[error("key ref missing: {0}")]
    KeyRefMissing(String),
    #[error("no-op migration refused")]
    NoOp,
    #[error("not supported: {what}")]
    NotSupported { what: String },
    #[error("unknown job kind: {0}")]
    UnknownKind(String),
    #[error("job not found")]
    NotFound,
    #[error("invalid job state")]
    InvalidState,
    #[error("cancelled")]
    Cancelled,
    #[error("io: {0}")]
    Io(String),
}

impl MigrateError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::MigrateSlice10Required => "MigrateSlice10Required",
            Self::NameExists { .. } => "NameExists",
            Self::QuotaExceeded => "QuotaExceeded",
            Self::ConstraintUnsatisfiable => "ConstraintUnsatisfiable",
            Self::MappingQueryRequired => "MappingQueryRequired",
            Self::MappingSourceMissing { .. } => "MappingSourceMissing",
            Self::JobInProgress => "JobInProgress",
            Self::AuthzDenied => "AuthzDenied",
            Self::StateUnreadable => "StateUnreadable",
            Self::KeyRefMissing(_) => "KeyRefMissing",
            Self::NoOp => "NoOp",
            Self::NotSupported { .. } => "NotSupported",
            Self::UnknownKind(_) => "UnknownKind",
            Self::NotFound => "NotFound",
            Self::InvalidState => "invalid_state",
            Self::Cancelled => "cancelled",
            Self::Io(_) => "io",
        }
    }
}

impl From<spacestorage_backup::BackupError> for MigrateError {
    fn from(e: spacestorage_backup::BackupError) -> Self {
        match e {
            spacestorage_backup::BackupError::BackupSlice10Required => {
                Self::MigrateSlice10Required
            }
            spacestorage_backup::BackupError::KeyRefMissing { key_ref } => {
                Self::KeyRefMissing(key_ref)
            }
            spacestorage_backup::BackupError::RestoreDestinationHasContent { container } => {
                Self::NameExists { container }
            }
            other => Self::Io(other.to_string()),
        }
    }
}
