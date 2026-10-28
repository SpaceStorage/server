//! Node adapter for `spacestorage-handler-postgresql`.

use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use spacestorage_handler_postgresql::{PostgresqlHandler, UserStore};
use spacestorage_types::ContainerCatalog;
use tokio_util::sync::CancellationToken;

use crate::handler::{ClientStream, Handler};

pub struct PostgresqlPortHandler {
    inner: PostgresqlHandler,
}

impl PostgresqlPortHandler {
    pub fn new(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self::with_release(catalog, builtin_release())
    }

    pub fn with_release(catalog: Arc<RwLock<ContainerCatalog>>, release: &str) -> Self {
        Self {
            inner: PostgresqlHandler::with_release(catalog, Arc::new(UserStore::demo()), release),
        }
    }

    pub fn with_users(catalog: Arc<RwLock<ContainerCatalog>>, users: Arc<UserStore>) -> Self {
        Self {
            inner: PostgresqlHandler::with_release(catalog, users, builtin_release()),
        }
    }
}

/// Dialect name for this node's Cargo features (`016` release profile).
fn builtin_release() -> &'static str {
    if cfg!(feature = "complete-product") {
        "complete-product"
    } else if cfg!(feature = "handlers-complete") {
        "handlers-complete"
    } else {
        "first-binary"
    }
}

#[async_trait]
impl Handler for PostgresqlPortHandler {
    fn name(&self) -> &str {
        "postgresql"
    }

    fn kind(&self) -> &str {
        "protocol"
    }

    fn owner(&self) -> &str {
        "02-protocol-drivers"
    }

    async fn serve(&self, stream: ClientStream, cancel: CancellationToken) {
        self.inner.serve(stream, cancel).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_handler_uses_build_release_profile() {
        let cat = Arc::new(RwLock::new(spacestorage_types::ContainerCatalog::new()));
        let h = PostgresqlPortHandler::new(cat);
        assert_eq!(h.inner.profile.as_str(), builtin_release());
    }
}
