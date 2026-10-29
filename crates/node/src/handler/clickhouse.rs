//! Node adapters for ClickHouse native + HTTP.

use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use spacestorage_compat::LimitServices;
use spacestorage_handler_clickhouse::{ClickHouseHttpHandler, ClickHouseNativeHandler};
use spacestorage_tenancy::NamespaceControl;
use spacestorage_types::ContainerCatalog;
use tokio_util::sync::CancellationToken;

use crate::handler::controlplane_namespaces::ControlPlaneNamespaces;
use crate::handler::{ClientStream, Handler};

pub struct ClickHousePortHandler {
    inner: ClickHouseNativeHandler,
}

impl ClickHousePortHandler {
    pub fn new(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self::with_limits(catalog, LimitServices::built_in())
    }

    pub fn with_limits(
        catalog: Arc<RwLock<ContainerCatalog>>,
        limit_services: LimitServices,
    ) -> Self {
        speak_native(ClickHouseNativeHandler::with_limits(
            catalog,
            limit_services,
        ))
    }

    pub fn with_tenancy(
        catalog: Arc<RwLock<ContainerCatalog>>,
        limit_services: LimitServices,
        tenancy: Arc<spacestorage_tenancy::Tenancy>,
    ) -> Self {
        speak_native(
            ClickHouseNativeHandler::with_limits(catalog, limit_services).with_tenancy(tenancy),
        )
    }

    pub fn with_namespace_control(
        catalog: Arc<RwLock<ContainerCatalog>>,
        limit_services: LimitServices,
        controlplane: Arc<
            tokio::sync::RwLock<Option<Arc<spacestorage_controlplane::ControlPlane>>>,
        >,
        tenancy: Arc<spacestorage_tenancy::Tenancy>,
    ) -> Self {
        let control: Arc<dyn NamespaceControl> = Arc::new(ControlPlaneNamespaces {
            controlplane,
            tenancy: Arc::clone(&tenancy),
            catalog: Arc::clone(&catalog),
        });
        speak_native(
            ClickHouseNativeHandler::with_limits(catalog, limit_services)
                .with_tenancy(tenancy)
                .with_namespace_control(control),
        )
    }
}

fn speak_native(mut inner: ClickHouseNativeHandler) -> ClickHousePortHandler {
    inner.profile = super::client_dialect();
    ClickHousePortHandler { inner }
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

    fn dialect(&self) -> Option<spacestorage_compat::DialectProfile> {
        Some(self.inner.profile)
    }
}

pub struct ClickHouseHttpPortHandler {
    inner: ClickHouseHttpHandler,
}

impl ClickHouseHttpPortHandler {
    pub fn new(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self::with_limits(catalog, LimitServices::built_in())
    }

    pub fn with_limits(
        catalog: Arc<RwLock<ContainerCatalog>>,
        limit_services: LimitServices,
    ) -> Self {
        speak_http(ClickHouseHttpHandler::with_limits(catalog, limit_services))
    }

    pub fn with_tenancy(
        catalog: Arc<RwLock<ContainerCatalog>>,
        limit_services: LimitServices,
        tenancy: Arc<spacestorage_tenancy::Tenancy>,
    ) -> Self {
        speak_http(
            ClickHouseHttpHandler::with_limits(catalog, limit_services).with_tenancy(tenancy),
        )
    }

    pub fn with_namespace_control(
        catalog: Arc<RwLock<ContainerCatalog>>,
        limit_services: LimitServices,
        controlplane: Arc<
            tokio::sync::RwLock<Option<Arc<spacestorage_controlplane::ControlPlane>>>,
        >,
        tenancy: Arc<spacestorage_tenancy::Tenancy>,
    ) -> Self {
        let control: Arc<dyn NamespaceControl> = Arc::new(ControlPlaneNamespaces {
            controlplane,
            tenancy: Arc::clone(&tenancy),
            catalog: Arc::clone(&catalog),
        });
        speak_http(
            ClickHouseHttpHandler::with_limits(catalog, limit_services)
                .with_tenancy(tenancy)
                .with_namespace_control(control),
        )
    }
}

fn speak_http(mut inner: ClickHouseHttpHandler) -> ClickHouseHttpPortHandler {
    inner.profile = super::client_dialect();
    ClickHouseHttpPortHandler { inner }
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

    fn dialect(&self) -> Option<spacestorage_compat::DialectProfile> {
        Some(self.inner.profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spacestorage_compat::DialectProfile;
    use spacestorage_handler_clickhouse::{ClickHouseReply, SessionState, dispatch};
    use spacestorage_types::ContainerCatalog;

    #[test]
    fn both_ports_run_group_by() {
        let native_cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let http_cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let native = ClickHousePortHandler::new(Arc::clone(&native_cat));
        let http = ClickHouseHttpPortHandler::new(Arc::clone(&http_cat));
        assert_eq!(native.dialect(), Some(DialectProfile::CompleteProduct));
        assert_eq!(http.dialect(), Some(DialectProfile::CompleteProduct));
        for (profile, mut session) in [
            (
                native.dialect().unwrap(),
                SessionState::demo_native(native_cat),
            ),
            (http.dialect().unwrap(), SessionState::demo_http(http_cat)),
        ] {
            session.profile = profile;
            assert!(matches!(
                dispatch(&mut session, "CREATE", &["TABLE", "t"]),
                ClickHouseReply::Ok
            ));
            assert!(matches!(
                dispatch(&mut session, "INSERT", &["t", "1", r#"{"id":"a","n":1}"#]),
                ClickHouseReply::Ok
            ));
            match dispatch(&mut session, "GROUP_BY", &["t", "id", "n"]) {
                ClickHouseReply::Rows(_) | ClickHouseReply::Ok => {}
                other => panic!("{other:?}"),
            }
        }
    }
}
