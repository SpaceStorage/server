//! Join-secret epochs with constant-time verify.

use crate::error::{MembershipError, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::path::Path;
use subtle::ConstantTimeEq;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretEpoch {
    pub epoch: u64,
    /// Hex-encoded opaque secret bytes (never log raw).
    pub secret_hex: String,
    pub accepted: bool,
}

impl SecretEpoch {
    pub fn secret_bytes(&self) -> Result<Vec<u8>> {
        hex::decode(&self.secret_hex).map_err(|e| MembershipError::Io(e.to_string()))
    }
}

/// Generate ≥32-byte secret and return epoch 1 accepted.
pub fn generate_bootstrap_secret() -> SecretEpoch {
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    SecretEpoch {
        epoch: 1,
        secret_hex: hex::encode(buf),
        accepted: true,
    }
}

/// Constant-time verify against all `accepted=true` epochs.
pub fn verify_secret(epochs: &[SecretEpoch], presented: &[u8]) -> bool {
    let mut ok = false;
    for e in epochs.iter().filter(|e| e.accepted) {
        if let Ok(bytes) = e.secret_bytes() {
            if bytes.len() == presented.len()
                && bool::from(bytes.as_slice().ct_eq(presented))
            {
                ok = true;
            } else if bytes.len() != presented.len() {
                // Length mismatch: still touch a dummy compare path.
                let _ = presented.ct_eq(presented);
            }
        }
    }
    ok
}

/// Write join secret to `token_file` with mode 0600 (spawn_blocking fsync).
pub async fn write_token_file(path: &Path, secret_hex: &str) -> Result<()> {
    let path = path.to_path_buf();
    let secret_hex = secret_hex.to_string();
    tokio::task::spawn_blocking(move || {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| MembershipError::Io(e.to_string()))?;
        }
        std::fs::write(&path, secret_hex.as_bytes())
            .map_err(|e| MembershipError::Io(e.to_string()))?;
        let f = std::fs::File::open(&path).map_err(|e| MembershipError::Io(e.to_string()))?;
        f.sync_all().map_err(|e| MembershipError::Io(e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = f
                .metadata()
                .map_err(|e| MembershipError::Io(e.to_string()))?
                .permissions();
            perms.set_mode(0o600);
            std::fs::set_permissions(&path, perms)
                .map_err(|e| MembershipError::Io(e.to_string()))?;
        }
        Ok(())
    })
    .await
    .map_err(|e| MembershipError::Io(e.to_string()))?
}

pub async fn read_token_file(path: &Path) -> Result<Vec<u8>> {
    let text = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| MembershipError::Io(e.to_string()))?;
    let trimmed = text.trim();
    // Accept raw hex or raw bytes written as hex string.
    if let Ok(bytes) = hex::decode(trimmed) {
        Ok(bytes)
    } else {
        Ok(trimmed.as_bytes().to_vec())
    }
}

/// Begin rotation: append new accepted epoch (overlap).
pub fn rotate_begin(epochs: &mut Vec<SecretEpoch>) -> SecretEpoch {
    let next = epochs.iter().map(|e| e.epoch).max().unwrap_or(0) + 1;
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    let ep = SecretEpoch {
        epoch: next,
        secret_hex: hex::encode(buf),
        accepted: true,
    };
    epochs.push(ep.clone());
    ep
}

/// Complete rotation: keep only the newest epoch accepted.
pub fn rotate_complete(epochs: &mut Vec<SecretEpoch>) {
    if let Some(max) = epochs.iter().map(|e| e.epoch).max() {
        for e in epochs.iter_mut() {
            e.accepted = e.epoch == max;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_verify_accepts_bootstrap() {
        let ep = generate_bootstrap_secret();
        let bytes = ep.secret_bytes().unwrap();
        assert!(verify_secret(&[ep.clone()], &bytes));
        assert!(!verify_secret(&[ep], b"wrong-secret-bytes-here!!!!!!"));
    }

    #[test]
    fn overlap_accepts_old_and_new() {
        let mut epochs = vec![generate_bootstrap_secret()];
        let old = epochs[0].secret_bytes().unwrap();
        let new = rotate_begin(&mut epochs);
        let new_bytes = new.secret_bytes().unwrap();
        assert!(verify_secret(&epochs, &old));
        assert!(verify_secret(&epochs, &new_bytes));
        rotate_complete(&mut epochs);
        assert!(!verify_secret(&epochs, &old));
        assert!(verify_secret(&epochs, &new_bytes));
    }
}
