//! Encryption declaration attach (no key material).

use crate::error::{Result, TenancyError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EncryptionDeclaration {
    pub algorithm: String,
    pub key_ref: String,
    pub scope: String,
}

impl EncryptionDeclaration {
    pub fn validate(algorithm: &str, key_ref: &str, scope: &str, storage_mode: &str) -> Result<Self> {
        if key_ref.is_empty() || key_ref.contains('\0') {
            return Err(TenancyError::KeyMaterialForbidden);
        }
        // Heuristic: inline hex key material refused.
        if key_ref.len() == 64 && key_ref.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(TenancyError::KeyMaterialForbidden);
        }
        let algorithm = match algorithm {
            "" | "AES-256-GCM" | "aes-256-gcm" => "AES-256-GCM".into(),
            "ChaCha20-Poly1305" | "chacha20-poly1305" => "ChaCha20-Poly1305".into(),
            other => other.to_string(),
        };
        if scope == "drives" && storage_mode == "memory" {
            return Err(TenancyError::EncryptionScopeInvalid {
                mode: storage_mode.into(),
                scope: scope.into(),
            });
        }
        Ok(Self {
            algorithm,
            key_ref: key_ref.into(),
            scope: scope.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_inline_hex() {
        let hex = "a".repeat(64);
        assert!(matches!(
            EncryptionDeclaration::validate("AES-256-GCM", &hex, "drives", "persistent"),
            Err(TenancyError::KeyMaterialForbidden)
        ));
    }
}
