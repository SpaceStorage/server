//! Node adapters for ClickHouse native + HTTP.

use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use spacestorage_handler_clickhouse::{ClickHouseHttpHandler, ClickHouseNativeHandler};
use spacestorage_types::ContainerCatalog;
use tokio_util::sync::CancellationToken;

use crate::handler::{ClientStream, Handler};

pub struct ClickHousePortHandler {
    inner: ClickHouseNativeHandler,
}

impl ClickHousePortHandler {
    pub fn new(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self {
            inner: ClickHouseNativeHandler::with_demo(catalog),
        }
    }
}

#[async_trait]
impl Handler for ClickHousePortHandler {
    fn name(&self) -> &str {
        "clickhouse"
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

pub struct ClickHouseHttpPortHandler {
    inner: ClickHouseHttpHandler,
}

impl ClickHouseHttpPortHandler {
    pub fn new(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self {
            inner: ClickHouseHttpHandler::with_demo(catalog),
        }
    }
}

#[async_trait]
impl Handler for ClickHouseHttpPortHandler {
    fn name(&self) -> &str {
        "clickhouse-http"
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
