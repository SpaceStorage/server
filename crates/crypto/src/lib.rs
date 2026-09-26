//! Envelope crypto, master-key file, and KeyAuthority (003/014).

pub mod aead;
pub mod envelope;
pub mod key_authority;
pub mod keyring_file;
pub mod master_file;

pub use envelope::{AeadAlgorithm, DataKey, KekRecord};
pub use key_authority::{EnvelopeAuthority, KeyAuthority, KeyError, SharedKeyAuthority};
pub use master_file::{MasterKey, MasterKeyError};

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use zeroize::Zeroizing;

    #[tokio::test]
    async fn master_wrap_unwrap_kek_and_data_key() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("master.key");
        let master = MasterKey::generate_and_write(&path).await.unwrap();
        let ns = uuid::Uuid::new_v4();
        let kek = envelope::generate_kek();
        let wrapped = envelope::wrap_kek(&master, ns, 1, &kek).unwrap();
        let unwrapped = envelope::unwrap_kek(&master, &wrapped).unwrap();
        assert_eq!(unwrapped.as_slice(), kek.as_slice());

        let data = envelope::generate_data_key();
        let dk = envelope::wrap_data_key(&unwrapped, "k1", AeadAlgorithm::Aes256Gcm, 1, &data)
            .unwrap();
        let resolved = envelope::unwrap_data_key(&unwrapped, &dk).unwrap();
        assert_eq!(resolved.as_slice(), data.as_slice());
        let _: Zeroizing<[u8; 32]> = resolved;
    }
}
