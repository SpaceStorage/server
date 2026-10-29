//! Node adapter for Elasticsearch HTTP handler.

use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use spacestorage_compat::LimitServices;
use spacestorage_handler_elasticsearch::ElasticsearchHandler;
use spacestorage_types::ContainerCatalog;
use tokio_util::sync::CancellationToken;

use crate::handler::{ClientStream, Handler};

pub struct ElasticsearchPortHandler {
    inner: ElasticsearchHandler,
}

impl ElasticsearchPortHandler {
    pub fn new(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self::with_limits(catalog, LimitServices::built_in())
    }

    pub fn with_limits(
        catalog: Arc<RwLock<ContainerCatalog>>,
        limit_services: LimitServices,
    ) -> Self {
        speak(ElasticsearchHandler::with_limits(catalog, limit_services))
    }

    pub fn with_tenancy(
        catalog: Arc<RwLock<ContainerCatalog>>,
        limit_services: LimitServices,
        tenancy: Arc<spacestorage_tenancy::Tenancy>,
    ) -> Self {
        speak(ElasticsearchHandler::with_limits(catalog, limit_services).with_tenancy(tenancy))
    }
}

fn speak(mut inner: ElasticsearchHandler) -> ElasticsearchPortHandler {
    inner.profile = super::client_dialect();
    ElasticsearchPortHandler { inner }
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

    fn dialect(&self) -> Option<spacestorage_compat::DialectProfile> {
        Some(self.inner.profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spacestorage_compat::DialectProfile;
    use spacestorage_handler_elasticsearch::{EsReply, SessionState, dispatch};
    use spacestorage_types::ContainerCatalog;

    #[test]
    fn port_speaks_complete_product_aggregations() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let h = ElasticsearchPortHandler::new(Arc::clone(&cat));
        assert_eq!(h.dialect(), Some(DialectProfile::CompleteProduct));
        let mut session = SessionState::demo(cat);
        session.profile = h.dialect().unwrap();
        assert!(matches!(
            dispatch(&mut session, "CREATE_INDEX", &["docs"]),
            EsReply::Ok(_)
        ));
        assert!(matches!(
            dispatch(&mut session, "INDEX", &["docs", "1", r#"{"n":3}"#]),
            EsReply::Ok(_)
        ));
        match dispatch(&mut session, "AGG_TERMS", &["docs", "n"]) {
            EsReply::Ok(body) => assert!(body.get("aggregations").is_some(), "{body}"),
            other => panic!("{other:?}"),
        }
    }
}
