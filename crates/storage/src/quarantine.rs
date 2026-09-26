//! Quarantine corrupt WAL segments: `{path}.corrupt.{unix_ts}`.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::StorageError;

fn dest_for(path: &Path) -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    PathBuf::from(format!("{}.corrupt.{}", path.display(), ts))
}

/// Rename `path` to `{path}.corrupt.{unix_ts}` (blocking; use on spawn_blocking).
pub fn quarantine_sync(path: &Path) -> Result<PathBuf, StorageError> {
    let dest = dest_for(path);
    std::fs::rename(path, &dest)?;
    Ok(dest)
}

pub async fn quarantine(path: &Path) -> Result<PathBuf, StorageError> {
    let from = path.to_path_buf();
    tokio::task::spawn_blocking(move || quarantine_sync(&from))
        .await
        .map_err(|e| StorageError::Join(e.to_string()))?
}
