//! Node adapter for S3 handler.

use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use spacestorage_handler_s3::S3Handler;
use spacestorage_types::ContainerCatalog;
use tokio_util::sync::CancellationToken;

use crate::handler::{ClientStream, Handler};

pub struct S3PortHandler {
    inner: S3Handler,
}

impl S3PortHandler {
    pub fn new(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self {
            inner: S3Handler::with_demo(catalog),
        }
    }
}

#[async_trait]
impl Handler for S3PortHandler {
    fn name(&self) -> &str {
        "s3"
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
