//! Master-key file IO: exactly 32 bytes, mode 0600, never logged.

use std::path::{Path, PathBuf};
use thiserror::Error;
use zeroize::{Zeroize, Zeroizing};

pub const MASTER_KEY_LEN: usize = 32;

#[derive(Debug, Error)]
pub enum MasterKeyError {
    #[error("master key required")]
    MasterKeyRequired,
    #[error("master key permissions must be 0600")]
    MasterKeyPermissions,
    #[error("master key must be exactly {MASTER_KEY_LEN} bytes")]
    MasterKeySize,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Cluster master key material. Never implements Debug/Display.
pub struct MasterKey {
    bytes: Zeroizing<[u8; MASTER_KEY_LEN]>,
    path: PathBuf,
}

impl MasterKey {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn bytes(&self) -> &[u8; MASTER_KEY_LEN] {
        &self.bytes
    }

    /// Async load via `spawn_blocking` (Constitution II — no sync fs on Tokio workers).
    pub async fn load(path: impl Into<PathBuf>) -> Result<Self, MasterKeyError> {
        let path = path.into();
        tokio::task::spawn_blocking(move || Self::load_blocking(&path))
            .await
            .map_err(|e| MasterKeyError::Io(std::io::Error::other(e)))?
    }

    pub fn load_blocking(path: &Path) -> Result<Self, MasterKeyError> {
        if !path.exists() {
            return Err(MasterKeyError::MasterKeyRequired);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(path)?;
            let mode = meta.permissions().mode() & 0o777;
            if mode != 0o600 {
                return Err(MasterKeyError::MasterKeyPermissions);
            }
        }
        let raw = std::fs::read(path)?;
        if raw.len() != MASTER_KEY_LEN {
            return Err(MasterKeyError::MasterKeySize);
        }
        let mut arr = [0u8; MASTER_KEY_LEN];
        arr.copy_from_slice(&raw);
        Ok(Self {
            bytes: Zeroizing::new(arr),
            path: path.to_path_buf(),
        })
    }

    /// Load existing master, or generate when `create_if_absent` and the path is missing.
    /// Without the flag (or when the file exists), behaves like [`Self::load`].
    pub async fn load_or_create(
        path: impl Into<PathBuf>,
        create_if_absent: bool,
    ) -> Result<Self, MasterKeyError> {
        let path = path.into();
        tokio::task::spawn_blocking(move || Self::load_or_create_blocking(&path, create_if_absent))
            .await
            .map_err(|e| MasterKeyError::Io(std::io::Error::other(e)))?
    }

    pub fn load_or_create_blocking(
        path: &Path,
        create_if_absent: bool,
    ) -> Result<Self, MasterKeyError> {
        if path.exists() {
            return Self::load_blocking(path);
        }
        if create_if_absent {
            return Self::generate_and_write_blocking(path);
        }
        Err(MasterKeyError::MasterKeyRequired)
    }

    /// Create a random 32-byte master key at `path` with mode 0600.
    pub async fn generate_and_write(path: impl Into<PathBuf>) -> Result<Self, MasterKeyError> {
        let path = path.into();
        tokio::task::spawn_blocking(move || Self::generate_and_write_blocking(&path))
            .await
            .map_err(|e| MasterKeyError::Io(std::io::Error::other(e)))?
    }

    pub fn generate_and_write_blocking(path: &Path) -> Result<Self, MasterKeyError> {
        use rand::RngCore;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut arr = [0u8; MASTER_KEY_LEN];
        rand::thread_rng().fill_bytes(&mut arr);
        std::fs::write(path, arr)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(path)?.permissions();
            perms.set_mode(0o600);
            std::fs::set_permissions(path, perms)?;
        }
        let key = Self {
            bytes: Zeroizing::new(arr),
            path: path.to_path_buf(),
        };
        // arr moved into Zeroizing; ensure no leftover
        let mut wipe = [0u8; MASTER_KEY_LEN];
        wipe.zeroize();
        Ok(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn rejects_wrong_size() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("mk");
        std::fs::write(&path, [0u8; 16]).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = std::fs::metadata(&path).unwrap().permissions();
            p.set_mode(0o600);
            std::fs::set_permissions(&path, p).unwrap();
        }
        let err = match MasterKey::load(&path).await {
            Err(e) => e,
            Ok(_) => panic!("expected MasterKeySize"),
        };
        assert!(matches!(err, MasterKeyError::MasterKeySize));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn rejects_wrong_mode() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("mk");
        std::fs::write(&path, [7u8; 32]).unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut p = std::fs::metadata(&path).unwrap().permissions();
        p.set_mode(0o644);
        std::fs::set_permissions(&path, p).unwrap();
        let err = match MasterKey::load(&path).await {
            Err(e) => e,
            Ok(_) => panic!("expected MasterKeyPermissions"),
        };
        assert!(matches!(err, MasterKeyError::MasterKeyPermissions));
    }

    #[tokio::test]
    async fn roundtrip_generate() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("mk");
        let a = MasterKey::generate_and_write(&path).await.unwrap();
        let b = MasterKey::load(&path).await.unwrap();
        assert_eq!(a.bytes(), b.bytes());
    }

    #[tokio::test]
    async fn load_or_create_generates_when_absent() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("mk");
        assert!(!path.exists());
        let a = MasterKey::load_or_create(&path, true).await.unwrap();
        assert!(path.exists());
        let b = MasterKey::load(&path).await.unwrap();
        assert_eq!(a.bytes(), b.bytes());
    }

    #[tokio::test]
    async fn load_or_create_requires_existing_without_flag() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("mk");
        let err = match MasterKey::load_or_create(&path, false).await {
            Err(e) => e,
            Ok(_) => panic!("expected MasterKeyRequired"),
        };
        assert!(matches!(err, MasterKeyError::MasterKeyRequired));
    }
}
