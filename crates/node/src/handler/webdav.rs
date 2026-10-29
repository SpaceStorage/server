//! Node adapter for WebDAV handler.

use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use spacestorage_compat::LimitServices;
use spacestorage_handler_webdav::WebDavHandler;
use spacestorage_types::ContainerCatalog;
use tokio_util::sync::CancellationToken;

use crate::handler::{ClientStream, Handler};

pub struct WebDavPortHandler {
    inner: WebDavHandler,
}

impl WebDavPortHandler {
    pub fn new(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self::with_limits(catalog, LimitServices::built_in())
    }

    pub fn with_limits(
        catalog: Arc<RwLock<ContainerCatalog>>,
        limit_services: LimitServices,
    ) -> Self {
        speak(WebDavHandler::with_limits(catalog, limit_services))
    }

    pub fn with_tenancy(
        catalog: Arc<RwLock<ContainerCatalog>>,
        limit_services: LimitServices,
        tenancy: Arc<spacestorage_tenancy::Tenancy>,
    ) -> Self {
        speak(WebDavHandler::with_limits(catalog, limit_services).with_tenancy(tenancy))
    }
}

fn speak(mut inner: WebDavHandler) -> WebDavPortHandler {
    inner.profile = super::client_dialect();
    WebDavPortHandler { inner }
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

    fn dialect(&self) -> Option<spacestorage_compat::DialectProfile> {
        Some(self.inner.profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spacestorage_compat::DialectProfile;
    use spacestorage_types::ContainerCatalog;

    #[test]
    fn port_speaks_complete_product() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let h = WebDavPortHandler::new(cat);
        assert_eq!(h.dialect(), Some(DialectProfile::CompleteProduct));
    }
}
