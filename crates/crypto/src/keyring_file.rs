//! Interim keyring_file provider — removed once EnvelopeAuthority is wired (014).
//! Present as a stub so 003 T002/T022 module layout compiles.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum KeyringError {
    #[error("keyring removed; use keys {{ master_key_file }}")]
    KeyringRemoved,
}

/// Loading `keys { keyring_file … }` is refused.
pub fn refuse_keyring_file() -> KeyringError {
    KeyringError::KeyringRemoved
}
