//! Node adapter for WebDAV handler.

use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use spacestorage_handler_webdav::WebDavHandler;
use spacestorage_types::ContainerCatalog;
use tokio_util::sync::CancellationToken;

use crate::handler::{ClientStream, Handler};

pub struct WebDavPortHandler {
    inner: WebDavHandler,
}

impl WebDavPortHandler {
    pub fn new(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self {
            inner: WebDavHandler::with_demo(catalog),
        }
    }
}

#[async_trait]
impl Handler for WebDavPortHandler {
    fn name(&self) -> &str {
        "webdav"
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
