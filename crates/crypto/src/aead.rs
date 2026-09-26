//! AEAD algorithm modules (003 T022 skeleton).

pub mod aes_gcm {
    //! AES-256-GCM — used by envelope wrap and sealed blocks.
    pub use aes_gcm::Aes256Gcm;
}

pub mod chacha {
    //! ChaCha20-Poly1305 — alternate data-key algorithm.
    pub use chacha20poly1305::ChaCha20Poly1305;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptionScope {
    Drives,
    DrivesAndMemory,
}

impl Default for EncryptionScope {
    fn default() -> Self {
        Self::Drives
    }
}
