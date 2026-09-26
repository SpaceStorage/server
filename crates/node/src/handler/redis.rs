//! Node adapter for `spacestorage-handler-redis`.

use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use spacestorage_handler_redis::RedisHandler;
use spacestorage_types::ContainerCatalog;
use tokio_util::sync::CancellationToken;

use crate::handler::{ClientStream, Handler};

pub struct RedisPortHandler {
    inner: RedisHandler,
}

impl RedisPortHandler {
    pub fn new(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self {
            inner: RedisHandler::with_demo(catalog),
        }
    }
}

#[async_trait]
impl Handler for RedisPortHandler {
    fn name(&self) -> &str {
        "redis"
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
