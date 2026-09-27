//! In-memory session and optional bearer token (014).

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use uuid::Uuid;

use crate::error::AuthzError;
use crate::principal::PrincipalId;

#[derive(Debug, Clone)]
pub struct Session {
    pub principal_id: PrincipalId,
    pub namespace_id: Option<Uuid>,
    pub credential_generation: u64,
    pub protocol: String,
}

impl Session {
    /// Later-request re-check seam (full logic in US1 T030).
    pub fn recheck(
        &self,
        enabled: bool,
        current_generation: u64,
    ) -> Result<(), AuthzError> {
        if !enabled {
            return Err(AuthzError::PrincipalDisabled);
        }
        if current_generation != self.credential_generation {
            return Err(AuthzError::AuthGenerationMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct SessionToken {
    pub token_hash: [u8; 32],
    pub principal_id: PrincipalId,
    pub generation: u64,
    pub login: String,
    pub cluster_admin: bool,
    pub expires_at: Instant,
}

/// In-process SessionToken registry (cluster-log apply deferred; T031 shape).
#[derive(Debug, Default)]
pub struct SessionTokenStore {
    by_hash: HashMap<[u8; 32], SessionToken>,
}

impl SessionTokenStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn hash_token(secret: &[u8]) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(secret);
        h.finalize().into()
    }

    /// Issue a random 32-byte token; stores SHA-256; returns (hex_token, record).
    pub fn issue(
        &mut self,
        principal_id: PrincipalId,
        login: impl Into<String>,
        cluster_admin: bool,
        generation: u64,
        ttl: Duration,
    ) -> (String, SessionToken) {
        let mut raw = [0u8; 32];
        getrandom_fill(&mut raw);
        let hex = hex_encode(&raw);
        let token_hash = Self::hash_token(&raw);
        let rec = SessionToken {
            token_hash,
            principal_id,
            generation,
            login: login.into(),
            cluster_admin,
            expires_at: Instant::now() + ttl,
        };
        self.by_hash.insert(token_hash, rec.clone());
        (hex, rec)
    }

    /// Resolve presented bearer (raw bytes or hex of 32-byte secret).
    pub fn resolve(&self, presented: &str) -> Option<SessionToken> {
        let presented = presented.trim();
        if presented.is_empty() {
            return None;
        }
        let hash = if let Ok(bytes) = hex_decode(presented) {
            if bytes.len() == 32 {
                Self::hash_token(&bytes)
            } else {
                Self::hash_token(presented.as_bytes())
            }
        } else {
            Self::hash_token(presented.as_bytes())
        };
        let rec = self.by_hash.get(&hash)?.clone();
        if Instant::now() >= rec.expires_at {
            return None;
        }
        Some(rec)
    }

    pub fn revoke_hash(&mut self, hash: &[u8; 32]) {
        self.by_hash.remove(hash);
    }
}

fn getrandom_fill(buf: &mut [u8]) {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    Uuid::now_v7().hash(&mut h);
    Instant::now().hash(&mut h);
    let mut n = h.finish();
    for b in buf.iter_mut() {
        *b = (n & 0xff) as u8;
        n = n.rotate_left(7) ^ 0x9e37_79b9_7f4a_7c15;
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

fn hex_decode(s: &str) -> Result<Vec<u8>, ()> {
    if s.len() % 2 != 0 {
        return Err(());
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = from_hex(bytes[i])?;
        let lo = from_hex(bytes[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Ok(out)
}

fn from_hex(b: u8) -> Result<u8, ()> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_and_resolve() {
        let mut store = SessionTokenStore::new();
        let pid = Uuid::now_v7();
        let (tok, _) = store.issue(pid, "admin", true, 1, Duration::from_secs(3600));
        let rec = store.resolve(&tok).expect("resolve");
        assert_eq!(rec.principal_id, pid);
        assert!(rec.cluster_admin);
        assert!(store.resolve("nope").is_none());
    }
}
