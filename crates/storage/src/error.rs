use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("sync none is not durable")]
    SyncNoneNotDurable,
    #[error("WAL format unsupported at {path}")]
    WalFormatUnsupported { path: String },
    #[error("format unsupported at {path}")]
    FormatUnsupported { path: String },
    #[error("disk full on drive {drive_id}")]
    DiskFull { drive_id: String },
    #[error("WAL corrupt: {0}")]
    WalCorrupt(String),
    #[error("PITR unmappable")]
    PitrUnmappable,
    #[error("restore destination has content")]
    RestoreDestinationHasContent,
    #[error("backup / PITR requires slice 10")]
    BackupSlice10Required,
    #[error("gc_grace too small")]
    GcGraceTooSmall,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("join: {0}")]
    Join(String),
}
