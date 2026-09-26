//! `KeyAuthority` trait and envelope implementation.

use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use zeroize::Zeroizing;

use crate::envelope::{self, DataKey, KekRecord};
use crate::master_file::MasterKey;

#[derive(Debug, Error)]
pub enum KeyError {
    #[error("key unresolvable: {0}")]
    KeyUnresolvable(String),
    /// Data-key unwrap refused: container is not hosted on this node (FR-008).
    #[error("key not hosted: {0}")]
    NotHosted(String),
    #[error("master key: {0}")]
    Master(#[from] crate::master_file::MasterKeyError),
    #[error("envelope: {0}")]
    Envelope(#[from] envelope::EnvelopeError),
}

#[async_trait]
pub trait KeyAuthority: Send + Sync {
    async fn resolve(&self, key_ref: &str) -> Result<Zeroizing<[u8; 32]>, KeyError>;
}

/// Master → namespace KEK → data key. Caches unwrapped data keys **only** for hosted containers.
pub struct EnvelopeAuthority {
    master: MasterKey,
    keks: RwLock<HashMap<uuid::Uuid, KekRecord>>,
    data_keys: RwLock<HashMap<String, DataKey>>,
    /// key_ref → namespace_id for unwrap path
    data_ns: RwLock<HashMap<String, uuid::Uuid>>,
    /// key_refs for containers this node hosts (FR-008 / contracts/keys.md).
    hosted: RwLock<HashSet<String>>,
    cache: RwLock<HashMap<String, Zeroizing<[u8; 32]>>>,
}

impl EnvelopeAuthority {
    pub fn new(master: MasterKey) -> Self {
        Self {
            master,
            keks: RwLock::new(HashMap::new()),
            data_keys: RwLock::new(HashMap::new()),
            data_ns: RwLock::new(HashMap::new()),
            hosted: RwLock::new(HashSet::new()),
            cache: RwLock::new(HashMap::new()),
        }
    }

    pub async fn put_kek(&self, record: KekRecord) {
        self.keks.write().await.insert(record.namespace_id, record);
    }

    pub async fn bind_data_key(&self, namespace_id: uuid::Uuid, dk: DataKey) {
        let key_ref = dk.key_ref.clone();
        self.data_keys.write().await.insert(key_ref.clone(), dk);
        self.data_ns.write().await.insert(key_ref, namespace_id);
        self.cache.write().await.clear();
    }

    /// Mark `key_ref` as hosted on this node (eligible for unwrap+cache via [`KeyAuthority::resolve`]).
    pub async fn mark_hosted(&self, key_ref: impl Into<String>) {
        self.hosted.write().await.insert(key_ref.into());
    }

    /// Remove hosted mark; drops any cached material for that key.
    pub async fn unmark_hosted(&self, key_ref: &str) {
        self.hosted.write().await.remove(key_ref);
        self.cache.write().await.remove(key_ref);
    }

    /// Replace the hosted set. Cache is cleared (hosting set changed).
    pub async fn set_hosted(&self, key_refs: impl IntoIterator<Item = impl Into<String>>) {
        let mut h = self.hosted.write().await;
        h.clear();
        h.extend(key_refs.into_iter().map(Into::into));
        self.cache.write().await.clear();
    }

    pub async fn is_hosted(&self, key_ref: &str) -> bool {
        self.hosted.read().await.contains(key_ref)
    }

    pub fn master(&self) -> &MasterKey {
        &self.master
    }

    pub async fn rotate_master(&mut self, new_master: MasterKey) -> Result<(), KeyError> {
        let mut keks: Vec<KekRecord> = self.keks.write().await.values().cloned().collect();
        envelope::rewrap_keks(&self.master, &new_master, &mut keks)?;
        let mut map = self.keks.write().await;
        map.clear();
        for r in keks {
            map.insert(r.namespace_id, r);
        }
        self.master = new_master;
        self.cache.write().await.clear();
        Ok(())
    }

    /// `CLUSTER_ADMIN` restore/admin unwrap: always unwraps, never reads or writes the hosted cache.
    pub async fn resolve_admin(&self, key_ref: &str) -> Result<Zeroizing<[u8; 32]>, KeyError> {
        self.unwrap_data_key(key_ref).await
    }

    async fn unwrap_data_key(&self, key_ref: &str) -> Result<Zeroizing<[u8; 32]>, KeyError> {
        let ns = self
            .data_ns
            .read()
            .await
            .get(key_ref)
            .copied()
            .ok_or_else(|| KeyError::KeyUnresolvable(key_ref.to_string()))?;
        let dk = self
            .data_keys
            .read()
            .await
            .get(key_ref)
            .cloned()
            .ok_or_else(|| KeyError::KeyUnresolvable(key_ref.to_string()))?;
        let kek_rec = self
            .keks
            .read()
            .await
            .get(&ns)
            .cloned()
            .ok_or_else(|| KeyError::KeyUnresolvable(key_ref.to_string()))?;
        let kek = envelope::unwrap_kek(&self.master, &kek_rec)?;
        Ok(envelope::unwrap_data_key(&kek, &dk)?)
    }
}

#[async_trait]
impl KeyAuthority for EnvelopeAuthority {
    async fn resolve(&self, key_ref: &str) -> Result<Zeroizing<[u8; 32]>, KeyError> {
        if !self.hosted.read().await.contains(key_ref) {
            return Err(KeyError::NotHosted(key_ref.to_string()));
        }
        if let Some(cached) = self.cache.read().await.get(key_ref) {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(cached.as_ref());
            return Ok(Zeroizing::new(arr));
        }
        let material = self.unwrap_data_key(key_ref).await?;
        {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(material.as_ref());
            self.cache
                .write()
                .await
                .insert(key_ref.to_string(), Zeroizing::new(arr));
        }
        Ok(material)
    }
}

/// Shared handle type used by storage/WAL encryption seams.
pub type SharedKeyAuthority = Arc<dyn KeyAuthority>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::{self, AeadAlgorithm};
    use crate::master_file::MasterKey;
    use tempfile::tempdir;

    async fn seeded_authority() -> (EnvelopeAuthority, String, Zeroizing<[u8; 32]>) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("master.key");
        let master = MasterKey::generate_and_write(&path).await.unwrap();
        let auth = EnvelopeAuthority::new(master);
        let ns = uuid::Uuid::new_v4();
        let kek = envelope::generate_kek();
        let wrapped = envelope::wrap_kek(auth.master(), ns, 1, &kek).unwrap();
        auth.put_kek(wrapped).await;
        let data = envelope::generate_data_key();
        let dk =
            envelope::wrap_data_key(&kek, "k-hosted", AeadAlgorithm::Aes256Gcm, 1, &data).unwrap();
        auth.bind_data_key(ns, dk).await;
        (auth, "k-hosted".to_string(), data)
    }

    #[tokio::test]
    async fn resolve_refuses_non_hosted() {
        let (auth, key_ref, _) = seeded_authority().await;
        let err = auth.resolve(&key_ref).await.unwrap_err();
        assert!(matches!(err, KeyError::NotHosted(_)));
    }

    #[tokio::test]
    async fn resolve_caches_only_when_hosted() {
        let (auth, key_ref, expected) = seeded_authority().await;
        auth.mark_hosted(&key_ref).await;
        let a = auth.resolve(&key_ref).await.unwrap();
        assert_eq!(a.as_slice(), expected.as_slice());
        assert!(auth.cache.read().await.contains_key(&key_ref));
        let b = auth.resolve(&key_ref).await.unwrap();
        assert_eq!(b.as_slice(), expected.as_slice());
    }

    #[tokio::test]
    async fn resolve_admin_unwraps_without_cache() {
        let (auth, key_ref, expected) = seeded_authority().await;
        let a = auth.resolve_admin(&key_ref).await.unwrap();
        assert_eq!(a.as_slice(), expected.as_slice());
        assert!(auth.cache.read().await.is_empty());
        // still not hosted → regular resolve refuses
        assert!(matches!(
            auth.resolve(&key_ref).await.unwrap_err(),
            KeyError::NotHosted(_)
        ));
    }

    #[tokio::test]
    async fn unmark_hosted_drops_cache() {
        let (auth, key_ref, _) = seeded_authority().await;
        auth.mark_hosted(&key_ref).await;
        auth.resolve(&key_ref).await.unwrap();
        assert!(auth.cache.read().await.contains_key(&key_ref));
        auth.unmark_hosted(&key_ref).await;
        assert!(!auth.cache.read().await.contains_key(&key_ref));
        assert!(matches!(
            auth.resolve(&key_ref).await.unwrap_err(),
            KeyError::NotHosted(_)
        ));
    }
}
