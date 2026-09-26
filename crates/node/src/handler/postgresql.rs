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
        Self {
            inner: PostgresqlHandler::with_demo_users(catalog),
        }
    }

    pub fn with_users(catalog: Arc<RwLock<ContainerCatalog>>, users: Arc<UserStore>) -> Self {
        Self {
            inner: PostgresqlHandler::new(catalog, users),
        }
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
