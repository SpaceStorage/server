//! Node adapter for Cassandra CQL handler.

use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use spacestorage_handler_cassandra::CassandraHandler;
use spacestorage_types::ContainerCatalog;
use tokio_util::sync::CancellationToken;

use crate::handler::{ClientStream, Handler};

pub struct CassandraPortHandler {
    inner: CassandraHandler,
}

impl CassandraPortHandler {
    pub fn new(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self {
            inner: CassandraHandler::with_demo(catalog),
        }
    }
}

#[async_trait]
impl Handler for CassandraPortHandler {
    fn name(&self) -> &str {
        "cassandra"
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
