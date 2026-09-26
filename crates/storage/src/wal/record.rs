//! WAL record framing per contracts/wal.md + format.md.

use crate::error::StorageError;
use uuid::Uuid;

pub const WAL_MAGIC: &[u8; 4] = b"WAL1";
pub const FORMAT_MAJOR: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WalRecordType {
    Put = 1,
    Delete = 2,
    TxnMarker = 3,
}

impl WalRecordType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::Put),
            2 => Some(Self::Delete),
            3 => Some(Self::TxnMarker),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WalRecord {
    pub lsn: u64,
    pub container_id: Uuid,
    pub stamp: u64,
    pub ty: WalRecordType,
    pub payload: Vec<u8>,
}

/// Segment header: magic WAL1 | format_major u16le | drive_id_len u16le | drive_id | node_id_len u16le | node_id
pub fn encode_segment_header(format_major: u16, drive_id: &str, node_id: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(WAL_MAGIC);
    out.extend_from_slice(&format_major.to_le_bytes());
    let db = drive_id.as_bytes();
    out.extend_from_slice(&(db.len() as u16).to_le_bytes());
    out.extend_from_slice(db);
    let nb = node_id.as_bytes();
    out.extend_from_slice(&(nb.len() as u16).to_le_bytes());
    out.extend_from_slice(nb);
    out
}

pub fn parse_segment_header(data: &[u8]) -> Result<(u16, String, String, usize), StorageError> {
    if data.len() < 8 {
        return Err(StorageError::WalFormatUnsupported {
            path: "<buffer>".into(),
        });
    }
    if &data[0..4] != WAL_MAGIC {
        return Err(StorageError::WalFormatUnsupported {
            path: "<buffer>".into(),
        });
    }
    let format_major = u16::from_le_bytes(data[4..6].try_into().unwrap());
    if format_major != FORMAT_MAJOR {
        return Err(StorageError::FormatUnsupported {
            path: format!("format_major={format_major}"),
        });
    }
    let mut i = 6;
    let dlen = u16::from_le_bytes(data[i..i + 2].try_into().unwrap()) as usize;
    i += 2;
    if i + dlen > data.len() {
        return Err(StorageError::WalCorrupt("short drive_id".into()));
    }
    let drive_id = String::from_utf8_lossy(&data[i..i + dlen]).into_owned();
    i += dlen;
    if i + 2 > data.len() {
        return Err(StorageError::WalCorrupt("short node_id len".into()));
    }
    let nlen = u16::from_le_bytes(data[i..i + 2].try_into().unwrap()) as usize;
    i += 2;
    if i + nlen > data.len() {
        return Err(StorageError::WalCorrupt("short node_id".into()));
    }
    let node_id = String::from_utf8_lossy(&data[i..i + nlen]).into_owned();
    i += nlen;
    Ok((format_major, drive_id, node_id, i))
}

/// Record: u32le length | u32 crc32 | u16 format_major | u8 type | u64 lsn | uuid | u64 stamp | u32 payload_len | payload
/// `length` covers everything after the length field (crc through payload).
pub fn encode_record(rec: &WalRecord) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&FORMAT_MAJOR.to_le_bytes());
    body.push(rec.ty as u8);
    body.extend_from_slice(&rec.lsn.to_le_bytes());
    body.extend_from_slice(rec.container_id.as_bytes());
    body.extend_from_slice(&rec.stamp.to_le_bytes());
    body.extend_from_slice(&(rec.payload.len() as u32).to_le_bytes());
    body.extend_from_slice(&rec.payload);
    let crc = crc32fast::hash(&body);
    let mut out = Vec::with_capacity(8 + body.len());
    let len = (4 + body.len()) as u32; // crc + body
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&body);
    out
}

pub fn parse_records(data: &[u8], start: usize) -> Result<(Vec<WalRecord>, u64), StorageError> {
    let mut i = start;
    let mut out = Vec::new();
    let mut torn = 0u64;
    while i < data.len() {
        if i + 4 > data.len() {
            torn += 1;
            break;
        }
        let len = u32::from_le_bytes(data[i..i + 4].try_into().unwrap()) as usize;
        i += 4;
        if len < 4 || i + len > data.len() {
            torn += 1;
            break;
        }
        let frame = &data[i..i + len];
        let crc_stored = u32::from_le_bytes(frame[0..4].try_into().unwrap());
        let body = &frame[4..];
        let crc = crc32fast::hash(body);
        if crc != crc_stored {
            // Mid-log vs tail: if this is the last frame attempt, treat as torn;
            // if more bytes follow, corrupt.
            if i + len >= data.len() {
                torn += 1;
                break;
            }
            return Err(StorageError::WalCorrupt(format!("crc mismatch at offset {}", i - 4)));
        }
        if body.len() < 2 + 1 + 8 + 16 + 8 + 4 {
            torn += 1;
            break;
        }
        let mut b = 0;
        let fmt = u16::from_le_bytes(body[b..b + 2].try_into().unwrap());
        b += 2;
        if fmt != FORMAT_MAJOR {
            return Err(StorageError::FormatUnsupported {
                path: format!("record format_major={fmt}"),
            });
        }
        let ty = WalRecordType::from_u8(body[b]).ok_or_else(|| {
            StorageError::WalCorrupt(format!("bad type {}", body[b]))
        })?;
        b += 1;
        let lsn = u64::from_le_bytes(body[b..b + 8].try_into().unwrap());
        b += 8;
        let container_id = Uuid::from_slice(&body[b..b + 16])
            .map_err(|_| StorageError::WalCorrupt("bad uuid".into()))?;
        b += 16;
        let stamp = u64::from_le_bytes(body[b..b + 8].try_into().unwrap());
        b += 8;
        let plen = u32::from_le_bytes(body[b..b + 4].try_into().unwrap()) as usize;
        b += 4;
        if b + plen > body.len() {
            torn += 1;
            break;
        }
        let payload = body[b..b + plen].to_vec();
        out.push(WalRecord {
            lsn,
            container_id,
            stamp,
            ty,
            payload,
        });
        i += len;
    }
    Ok((out, torn))
}

/// Build AAD for WAL payload AEAD: `drive_id ‖ lsn ‖ container_id`.
pub fn wal_payload_aad(drive_id: &str, lsn: u64, container_id: Uuid) -> Vec<u8> {
    let mut aad = Vec::with_capacity(drive_id.len() + 8 + 16);
    aad.extend_from_slice(drive_id.as_bytes());
    aad.extend_from_slice(&lsn.to_le_bytes());
    aad.extend_from_slice(container_id.as_bytes());
    aad
}

/// Optionally encrypt payload with container data key (014 seam).
/// AAD MUST be `drive_id ‖ lsn ‖ container_id` (caller-supplied).
pub fn maybe_encrypt_payload(
    plaintext: &[u8],
    key: Option<&[u8; 32]>,
    aad: &[u8],
) -> Result<Vec<u8>, StorageError> {
    match key {
        None => Ok(plaintext.to_vec()),
        Some(k) => {
            use aes_gcm::{
                aead::{Aead, KeyInit},
                Aes256Gcm, Nonce,
            };
            use rand::RngCore;
            let cipher = Aes256Gcm::new_from_slice(k)
                .map_err(|e| StorageError::WalCorrupt(e.to_string()))?;
            let mut nonce = [0u8; 12];
            rand::thread_rng().fill_bytes(&mut nonce);
            let mut out = nonce.to_vec();
            let ct = cipher
                .encrypt(
                    Nonce::from_slice(&nonce),
                    aes_gcm::aead::Payload {
                        msg: plaintext,
                        aad,
                    },
                )
                .map_err(|_| StorageError::WalCorrupt("aead encrypt".into()))?;
            out.extend_from_slice(&ct);
            Ok(out)
        }
    }
}

/// Optionally decrypt WAL payload with container data key (014 seam / T065).
/// AAD MUST be `drive_id ‖ lsn ‖ container_id` (same as encrypt).
/// `None` key → treat payload as plaintext (unencrypted container).
pub fn maybe_decrypt_payload(
    payload: &[u8],
    key: Option<&[u8; 32]>,
    aad: &[u8],
) -> Result<Vec<u8>, StorageError> {
    match key {
        None => Ok(payload.to_vec()),
        Some(k) => {
            use aes_gcm::{
                aead::{Aead, KeyInit},
                Aes256Gcm, Nonce,
            };
            if payload.len() < 12 {
                return Err(StorageError::WalCorrupt("short aead ciphertext".into()));
            }
            let (nonce, ct) = payload.split_at(12);
            let cipher = Aes256Gcm::new_from_slice(k)
                .map_err(|e| StorageError::WalCorrupt(e.to_string()))?;
            cipher
                .decrypt(
                    Nonce::from_slice(nonce),
                    aes_gcm::aead::Payload { msg: ct, aad },
                )
                .map_err(|_| StorageError::WalCorrupt("aead decrypt".into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = [9u8; 32];
        let aad = wal_payload_aad("default", 42, Uuid::nil());
        let ct = maybe_encrypt_payload(b"hello", Some(&key), &aad).unwrap();
        assert_ne!(ct, b"hello");
        let pt = maybe_decrypt_payload(&ct, Some(&key), &aad).unwrap();
        assert_eq!(pt, b"hello");
        assert_eq!(
            maybe_decrypt_payload(b"plain", None, &aad).unwrap(),
            b"plain"
        );
    }
}
