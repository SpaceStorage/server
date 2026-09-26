//! Snapshot / PITR stubs — admin snapshot jobs require slice 10.

pub mod manifest;
pub mod pitr;
pub mod restore;
pub mod snapshot;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum BackupError {
    #[error("backup / PITR operator jobs require slice 10")]
    BackupSlice10Required,
}

pub struct SnapshotService;
impl SnapshotService {
    pub fn create(&self) -> Result<(), BackupError> {
        Err(BackupError::BackupSlice10Required)
    }
}

pub struct RestoreService;
impl RestoreService {
    pub fn restore_fill(&self) -> Result<(), BackupError> {
        Err(BackupError::BackupSlice10Required)
    }
}
