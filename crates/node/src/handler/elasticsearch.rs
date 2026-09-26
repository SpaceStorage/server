//! Node adapter for Elasticsearch HTTP handler.

use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use spacestorage_handler_elasticsearch::ElasticsearchHandler;
use spacestorage_types::ContainerCatalog;
use tokio_util::sync::CancellationToken;

use crate::handler::{ClientStream, Handler};

pub struct ElasticsearchPortHandler {
    inner: ElasticsearchHandler,
}

impl ElasticsearchPortHandler {
    pub fn new(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self {
            inner: ElasticsearchHandler::with_demo(catalog),
        }
    }
}

#[async_trait]
impl Handler for ElasticsearchPortHandler {
    fn name(&self) -> &str {
        "elasticsearch"
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
