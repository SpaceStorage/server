//! Master → KEK → data-key envelope helpers.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::master_file::MasterKey;

const NONCE_LEN: usize = 12;

#[derive(Debug, Error)]
pub enum EnvelopeError {
    #[error("aead failure")]
    Aead,
    #[error("key unresolvable: {0}")]
    KeyUnresolvable(String),
    #[error("bad wrapped blob")]
    BadBlob,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AeadAlgorithm {
    Aes256Gcm,
    Chacha20Poly1305,
}

impl Default for AeadAlgorithm {
    fn default() -> Self {
        Self::Aes256Gcm
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KekRecord {
    pub namespace_id: Uuid,
    /// nonce ‖ ciphertext (AES-256-GCM under master)
    pub wrapped: Vec<u8>,
    pub kek_epoch: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataKey {
    pub key_ref: String,
    pub algorithm: AeadAlgorithm,
    /// nonce ‖ ciphertext under namespace KEK
    pub wrapped: Vec<u8>,
    pub version: u32,
}

pub fn generate_kek() -> Zeroizing<[u8; 32]> {
    let mut b = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut b);
    Zeroizing::new(b)
}

pub fn generate_data_key() -> Zeroizing<[u8; 32]> {
    generate_kek()
}

fn aead_seal(key: &[u8; 32], aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, EnvelopeError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| EnvelopeError::Aead)?;
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let mut out = nonce_bytes.to_vec();
    let ct = cipher
        .encrypt(
            nonce,
            aes_gcm::aead::Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| EnvelopeError::Aead)?;
    out.extend_from_slice(&ct);
    Ok(out)
}

fn aead_open(key: &[u8; 32], aad: &[u8], blob: &[u8]) -> Result<Vec<u8>, EnvelopeError> {
    if blob.len() < NONCE_LEN + 16 {
        return Err(EnvelopeError::BadBlob);
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| EnvelopeError::Aead)?;
    let (nonce_bytes, ct) = blob.split_at(NONCE_LEN);
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher
        .decrypt(
            nonce,
            aes_gcm::aead::Payload {
                msg: ct,
                aad,
            },
        )
        .map_err(|_| EnvelopeError::Aead)
}

pub fn wrap_kek(
    master: &MasterKey,
    namespace_id: Uuid,
    kek_epoch: u64,
    kek: &[u8; 32],
) -> Result<KekRecord, EnvelopeError> {
    let aad = namespace_id.as_bytes();
    let wrapped = aead_seal(master.bytes(), aad, kek)?;
    Ok(KekRecord {
        namespace_id,
        wrapped,
        kek_epoch,
    })
}

pub fn unwrap_kek(master: &MasterKey, record: &KekRecord) -> Result<Zeroizing<[u8; 32]>, EnvelopeError> {
    let aad = record.namespace_id.as_bytes();
    let pt = aead_open(master.bytes(), aad, &record.wrapped)?;
    if pt.len() != 32 {
        return Err(EnvelopeError::BadBlob);
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&pt);
    Ok(Zeroizing::new(arr))
}

/// Rewrap every KEK under a new master; bump epoch; data keys unchanged.
pub fn rewrap_keks(
    old_master: &MasterKey,
    new_master: &MasterKey,
    records: &mut [KekRecord],
) -> Result<(), EnvelopeError> {
    for r in records.iter_mut() {
        let kek = unwrap_kek(old_master, r)?;
        let next_epoch = r.kek_epoch.saturating_add(1);
        *r = wrap_kek(new_master, r.namespace_id, next_epoch, &kek)?;
    }
    Ok(())
}

pub fn wrap_data_key(
    kek: &[u8; 32],
    key_ref: impl Into<String>,
    algorithm: AeadAlgorithm,
    version: u32,
    data_key: &[u8; 32],
) -> Result<DataKey, EnvelopeError> {
    let key_ref = key_ref.into();
    let aad = key_ref.as_bytes();
    let wrapped = aead_seal(kek, aad, data_key)?;
    Ok(DataKey {
        key_ref,
        algorithm,
        wrapped,
        version,
    })
}

pub fn unwrap_data_key(
    kek: &[u8; 32],
    dk: &DataKey,
) -> Result<Zeroizing<[u8; 32]>, EnvelopeError> {
    let pt = aead_open(kek, dk.key_ref.as_bytes(), &dk.wrapped)?;
    if pt.len() != 32 {
        return Err(EnvelopeError::BadBlob);
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&pt);
    Ok(Zeroizing::new(arr))
}
