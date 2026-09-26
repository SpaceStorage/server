//! PostgreSQL first-binary dialect (002) — async Handler::serve over wire 3.0.
//!
//! Classifies via `spacestorage_compat::classify_pg_verb` before catalog IR.
//! Startup uses SCRAM-SHA-256 ([`auth`]); SQL is auto-commit only.

mod auth;
mod serve;
mod sql;
mod wire;

pub use auth::UserStore;
pub use serve::{exec_sql_sync, serve};
pub use sql::{execute_sql, ExecError, ExecResult, SharedCatalog};
pub use wire::FEATURE_NOT_SUPPORTED;

pub const WIRE_VERSION: &str = "3.0";

use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::sync::CancellationToken;

/// Node-facing PostgreSQL handler (async serve only — Constitution II).
pub struct PostgresqlHandler {
    pub catalog: SharedCatalog,
    pub users: Arc<UserStore>,
}

impl PostgresqlHandler {
    pub fn new(catalog: SharedCatalog, users: Arc<UserStore>) -> Self {
        Self { catalog, users }
    }

    pub fn with_demo_users(catalog: SharedCatalog) -> Self {
        Self::new(catalog, Arc::new(UserStore::demo()))
    }

    /// Async request path used by `crates/node` `Handler::serve`.
    pub async fn serve<S>(&self, stream: S, cancel: CancellationToken)
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        serve(
            stream,
            cancel,
            Arc::clone(&self.catalog),
            spacestorage_compat::DialectProfile::FirstBinary,
            Arc::clone(&self.users),
        )
        .await;
    }

    /// Unit-test helper (no I/O). Prefer [`Self::serve`] for connections.
    pub fn exec_sql(&self, sql: &str) -> Result<(), (String, String)> {
        exec_sql_sync(&self.catalog, "default", sql)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spacestorage_compat::DialectProfile;
    use spacestorage_types::ContainerCatalog;
    use std::sync::RwLock;
    use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};

    #[test]
    fn begin_returns_0a000() {
        let h = PostgresqlHandler::with_demo_users(Arc::new(RwLock::new(ContainerCatalog::new())));
        let err = h.exec_sql("BEGIN").unwrap_err();
        assert_eq!(err.0, FEATURE_NOT_SUPPORTED);
    }

    #[test]
    fn update_delete_smoke() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let h = PostgresqlHandler::with_demo_users(Arc::clone(&cat));
        h.exec_sql("CREATE TABLE t (id text, v text)").unwrap();
        h.exec_sql("INSERT INTO t VALUES ('1', 'a')").unwrap();
        h.exec_sql("UPDATE t SET v = 'b' WHERE id = '1'").unwrap();
        h.exec_sql("DELETE FROM t WHERE id = '1'").unwrap();
        h.exec_sql("DROP TABLE t").unwrap();
    }

    #[tokio::test]
    async fn serve_startup_advertises_scram() {
        let catalog = Arc::new(RwLock::new(ContainerCatalog::new()));
        let h = PostgresqlHandler::with_demo_users(Arc::clone(&catalog));
        let (mut client, server) = duplex(8192);
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        let join = tokio::spawn(async move {
            h.serve(server, c2).await;
        });

        let mut startup = Vec::new();
        let mut body = Vec::new();
        body.extend_from_slice(&196608i32.to_be_bytes());
        body.extend_from_slice(b"user\0demo\0database\0demo\0\0");
        startup.extend_from_slice(&((4 + body.len()) as i32).to_be_bytes());
        startup.extend_from_slice(&body);
        client.write_all(&startup).await.unwrap();

        let mut buf = vec![0u8; 1024];
        let n = client.read(&mut buf).await.unwrap();
        assert!(n > 0);
        // AuthenticationSASL
        assert_eq!(buf[0], b'R');
        assert_eq!(i32::from_be_bytes(buf[5..9].try_into().unwrap()), 10);

        cancel.cancel();
        drop(client);
        let _ = join.await;
        let _ = DialectProfile::FirstBinary;
    }
}
