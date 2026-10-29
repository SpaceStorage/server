use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::sync::CancellationToken;

/// Accepted connection after optional TLS termination (FR-027: no plaintext fallback).
pub trait IoStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> IoStream for T {}

pub type ClientStream = Box<dyn IoStream>;

#[async_trait]
pub trait Handler: Send + Sync {
    fn name(&self) -> &str;
    fn kind(&self) -> &str {
        "protocol"
    }
    fn owner(&self) -> &str {
        "01-runtime-cli-api"
    }
    async fn serve(&self, stream: ClientStream, cancel: CancellationToken);

    /// Entrypoint-aware serve (syslog 1:1 bind). Default ignores the name.
    async fn serve_named(
        &self,
        stream: ClientStream,
        cancel: CancellationToken,
        _entrypoint: &str,
    ) {
        self.serve(stream, cancel).await
    }

    /// Dialect copied onto each new client session. `None` for non-protocol handlers.
    fn dialect(&self) -> Option<spacestorage_compat::DialectProfile> {
        None
    }

    /// One statement through the handler the node registered, on `session`.
    /// `None` when this handler has no SQL path.
    fn try_exec_sql(&self, _session: &str, _sql: &str) -> Option<Result<(), (String, String)>> {
        None
    }
}

/// Dialect every client protocol on this process speaks.
pub(crate) fn client_dialect() -> spacestorage_compat::DialectProfile {
    spacestorage_compat::profile_from_release(crate::CLIENT_RELEASE)
        .expect("CLIENT_RELEASE maps to a dialect")
}

pub struct HandlerRegistry {
    handlers: HashMap<String, Arc<dyn Handler>>,
}

impl HandlerRegistry {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    pub fn register(&mut self, h: Arc<dyn Handler>) -> Result<(), String> {
        let name = h.name().to_string();
        if self.handlers.contains_key(&name) {
            return Err(format!("duplicate handler '{name}'"));
        }
        self.handlers.insert(name, h);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Handler>> {
        self.handlers.get(name).cloned()
    }

    pub fn names(&self) -> Vec<String> {
        let mut v: Vec<_> = self.handlers.keys().cloned().collect();
        v.sort();
        v
    }
}

impl Default for HandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub mod admin_http;
pub mod admin_tcp;
pub mod console_backend;
pub mod controlplane_namespaces;
pub mod echo;
pub mod fabric;
pub mod live_map;
pub mod postgresql;
pub mod redis;
pub mod stub_cluster;

pub mod cassandra;
pub mod clickhouse;
pub mod elasticsearch;
pub mod s3;
pub mod syslog;
pub mod webdav;
