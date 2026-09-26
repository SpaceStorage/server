//! Spill directory lifecycle (005).

use crate::error::ExecError;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug)]
pub struct SpillDir {
    path: PathBuf,
    keep: bool,
}

impl SpillDir {
    pub fn create(data_dir: &Path, exec_id: Uuid) -> Result<Self, ExecError> {
        let path = data_dir.join("spill").join(exec_id.to_string());
        fs::create_dir_all(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::StorageFull {
                ExecError::Admission {
                    limit: "spill_disk".into(),
                    current: 0,
                    max: 0,
                }
            } else {
                ExecError::Msg(format!("spill_create: {e}"))
            }
        })?;
        Ok(Self { path, keep: false })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn keep_on_drop(mut self) {
        self.keep = true;
    }
}

impl Drop for SpillDir {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn spill_cleaned_on_drop() {
        let dir = tempdir().unwrap();
        let id = Uuid::now_v7();
        let spill = SpillDir::create(dir.path(), id).unwrap();
        let p = spill.path().to_path_buf();
        assert!(p.is_dir());
        drop(spill);
        assert!(!p.exists());
    }
}
