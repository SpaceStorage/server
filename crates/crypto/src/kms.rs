//! External KMS provider path (014 FR-008 later provider; intent 16).
//!
//! Master-key file remains the first-binary default. External KMS is an alternate
//! [`MasterProvider`] that unwraps the same envelope references without changing
//! KEK / data-key wire shapes.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use zeroize::Zeroizing;

use crate::master_file::{MasterKey, MasterKeyError};

#[derive(Debug, Error)]
pub enum KmsError {
    #[error("kms unavailable: {0}")]
    Unavailable(String),
    #[error("kms key not found: {0}")]
    KeyNotFound(String),
    #[error("kms auth failed")]
    AuthFailed,
    #[error("master: {0}")]
    Master(#[from] MasterKeyError),
    #[error("{0}")]
    Msg(String),
}

/// How the cluster resolves the wrapping master material.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MasterProviderConfig {
    /// Default first-binary path: 32-byte file mode ≤ 0600.
    MasterKeyFile { path: String },
    /// External KMS (HTTP JSON stub compatible with vault-like unwrap).
    ExternalKms {
        /// Base URL, e.g. `https://vault.example/v1/transit`.
        endpoint: String,
        /// Key name / transit path.
        key_name: String,
        /// Bearer / AppRole token (never logged).
        #[serde(default)]
        token: String,
    },
}

/// Resolve 32-byte master wrapping key material.
#[async_trait]
pub trait MasterProvider: Send + Sync {
    async fn resolve_master(&self) -> Result<Zeroizing<[u8; 32]>, KmsError>;
    fn provider_kind(&self) -> &'static str;
}

/// File-backed provider wrapping [`MasterKey`].
pub struct FileMasterProvider {
    key: MasterKey,
}

impl FileMasterProvider {
    pub fn new(key: MasterKey) -> Self {
        Self { key }
    }

    pub async fn from_path(path: impl AsRef<std::path::Path>) -> Result<Self, KmsError> {
        Ok(Self {
            key: MasterKey::load(path.as_ref().to_path_buf()).await?,
        })
    }
}

#[async_trait]
impl MasterProvider for FileMasterProvider {
    async fn resolve_master(&self) -> Result<Zeroizing<[u8; 32]>, KmsError> {
        Ok(Zeroizing::new(*self.key.bytes()))
    }

    fn provider_kind(&self) -> &'static str {
        "master_key_file"
    }
}

/// In-process / testable external KMS with optional live Vault-style HTTP unwrap.
pub struct ExternalKmsProvider {
    endpoint: String,
    key_name: String,
    token: String,
    /// Local cache of key material for offline / unit tests (simulates KMS unwrap).
    keys: RwLock<HashMap<String, [u8; 32]>>,
    /// When true, `resolve_master` prefers HTTP unwrap against `endpoint`.
    live_http: bool,
}

impl ExternalKmsProvider {
    pub fn new(endpoint: impl Into<String>, key_name: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            key_name: key_name.into(),
            token: token.into(),
            keys: RwLock::new(HashMap::new()),
            live_http: false,
        }
    }

    /// Enable live Vault-style HTTP unwrap (`GET {endpoint}/decrypt/{key_name}`).
    pub fn with_live_http(mut self) -> Self {
        self.live_http = true;
        self
    }

    pub fn from_config(cfg: &MasterProviderConfig) -> Result<Self, KmsError> {
        match cfg {
            MasterProviderConfig::ExternalKms {
                endpoint,
                key_name,
                token,
            } => Ok(Self::new(endpoint, key_name, token).with_live_http()),
            MasterProviderConfig::MasterKeyFile { .. } => Err(KmsError::Msg(
                "ExternalKmsProvider requires ExternalKms config".into(),
            )),
        }
    }

    /// Seed / rotate a named key (test harness and bootstrap).
    pub async fn put_key(&self, name: impl Into<String>, material: [u8; 32]) {
        self.keys.write().await.insert(name.into(), material);
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn key_name(&self) -> &str {
        &self.key_name
    }

    /// Auth gate: empty token refused (product surface).
    fn check_auth(&self) -> Result<(), KmsError> {
        if self.token.is_empty() {
            Err(KmsError::AuthFailed)
        } else {
            Ok(())
        }
    }

    /// Vault transit-like unwrap: `GET {endpoint}/decrypt/{key}` with Bearer token.
    /// Response JSON: `{"data":{"plaintext":"<base64 32 bytes>"}}` (Vault transit shape).
    async fn http_unwrap(&self) -> Result<Zeroizing<[u8; 32]>, KmsError> {
        self.check_auth()?;
        let url = format!(
            "{}/decrypt/{}",
            self.endpoint.trim_end_matches('/'),
            self.key_name
        );
        let parsed = url::Url::parse(&url).map_err(|e| KmsError::Msg(e.to_string()))?;
        let host = parsed.host_str().ok_or_else(|| KmsError::Msg("kms bad host".into()))?;
        let port = parsed.port_or_known_default().unwrap_or(80);
        let path = if parsed.path().is_empty() {
            "/"
        } else {
            parsed.path()
        };
        let addr = format!("{host}:{port}");
        let mut stream = tokio::net::TcpStream::connect(&addr)
            .await
            .map_err(|e| KmsError::Unavailable(e.to_string()))?;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let req = format!(
            "GET {path} HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
            self.token
        );
        stream
            .write_all(req.as_bytes())
            .await
            .map_err(|e| KmsError::Unavailable(e.to_string()))?;
        let mut buf = Vec::new();
        stream
            .read_to_end(&mut buf)
            .await
            .map_err(|e| KmsError::Unavailable(e.to_string()))?;
        let text = String::from_utf8_lossy(&buf);
        let body = text
            .split("\r\n\r\n")
            .nth(1)
            .ok_or_else(|| KmsError::Unavailable("kms empty body".into()))?;
        let v: serde_json::Value =
            serde_json::from_str(body).map_err(|e| KmsError::Msg(e.to_string()))?;
        let b64 = v
            .pointer("/data/plaintext")
            .and_then(|x| x.as_str())
            .ok_or_else(|| KmsError::KeyNotFound(self.key_name.clone()))?;
        let raw = base64_decode(b64).map_err(KmsError::Msg)?;
        if raw.len() != 32 {
            return Err(KmsError::Msg(format!(
                "kms plaintext len {} want 32",
                raw.len()
            )));
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&raw);
        Ok(Zeroizing::new(out))
    }
}

fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    // Minimal base64 decoder for 32-byte keys (no padding required).
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let mut buf = 0u32;
    let mut bits = 0u32;
    for &c in s.as_bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        let v = T
            .iter()
            .position(|&x| x == c)
            .ok_or_else(|| format!("bad base64 byte {c}"))? as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

fn base64_encode(bytes: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let mut n = (chunk[0] as u32) << 16;
        if chunk.len() > 1 {
            n |= (chunk[1] as u32) << 8;
        }
        if chunk.len() > 2 {
            n |= chunk[2] as u32;
        }
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[async_trait]
impl MasterProvider for ExternalKmsProvider {
    async fn resolve_master(&self) -> Result<Zeroizing<[u8; 32]>, KmsError> {
        self.check_auth()?;
        if self.live_http {
            match self.http_unwrap().await {
                Ok(m) => return Ok(m),
                Err(e) => {
                    // Fall back to local cache when HTTP unavailable (lab / offline).
                    tracing::debug!(error = %e, "kms http unwrap failed; trying local cache");
                }
            }
        }
        let keys = self.keys.read().await;
        let bytes = keys
            .get(&self.key_name)
            .copied()
            .ok_or_else(|| KmsError::KeyNotFound(self.key_name.clone()))?;
        let _ = &self.endpoint;
        Ok(Zeroizing::new(bytes))
    }

    fn provider_kind(&self) -> &'static str {
        "external_kms"
    }
}

/// Build a [`MasterProvider`] from config.
pub async fn open_master_provider(
    cfg: &MasterProviderConfig,
) -> Result<Arc<dyn MasterProvider>, KmsError> {
    match cfg {
        MasterProviderConfig::MasterKeyFile { path } => {
            Ok(Arc::new(FileMasterProvider::from_path(path).await?))
        }
        MasterProviderConfig::ExternalKms { .. } => {
            Ok(Arc::new(ExternalKmsProvider::from_config(cfg)?))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn file_provider_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("master.key");
        let mk = MasterKey::generate_and_write(&path).await.unwrap();
        let p = FileMasterProvider::new(mk);
        assert_eq!(p.provider_kind(), "master_key_file");
        let m = p.resolve_master().await.unwrap();
        assert_eq!(m.as_slice().len(), 32);
    }

    #[tokio::test]
    async fn external_kms_requires_token_and_key() {
        let kms = ExternalKmsProvider::new("https://kms.example/v1", "cluster-master", "");
        assert!(matches!(
            kms.resolve_master().await,
            Err(KmsError::AuthFailed)
        ));
        let kms = ExternalKmsProvider::new("https://kms.example/v1", "cluster-master", "s.token");
        assert!(matches!(
            kms.resolve_master().await,
            Err(KmsError::KeyNotFound(_))
        ));
        let material = [7u8; 32];
        kms.put_key("cluster-master", material).await;
        let got = kms.resolve_master().await.unwrap();
        assert_eq!(got.as_slice(), &material);
        assert_eq!(kms.provider_kind(), "external_kms");
    }

    #[tokio::test]
    async fn external_kms_live_http_vault_unwrap() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        let material = [9u8; 32];
        let b64 = base64_encode(&material);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 2048];
            let _ = sock.read(&mut buf).await;
            let body = format!(r#"{{"data":{{"plaintext":"{b64}"}}}}"#);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
        });
        let endpoint = format!("http://{}:{}", addr.ip(), addr.port());
        let kms = ExternalKmsProvider::new(endpoint, "cluster-master", "s.token").with_live_http();
        let got = kms.resolve_master().await.unwrap();
        assert_eq!(got.as_slice(), &material);
    }
}
