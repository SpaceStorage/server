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
        Self {
            inner: ClickHouseNativeHandler::with_limits(catalog, limit_services),
        }
    }

    pub fn with_tenancy(
        catalog: Arc<RwLock<ContainerCatalog>>,
        limit_services: LimitServices,
        tenancy: Arc<spacestorage_tenancy::Tenancy>,
    ) -> Self {
        Self {
            inner: ClickHouseNativeHandler::with_limits(catalog, limit_services)
                .with_tenancy(tenancy),
        }
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
        Self {
            inner: ClickHouseNativeHandler::with_limits(catalog, limit_services)
                .with_tenancy(tenancy)
                .with_namespace_control(control),
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
        Self::with_limits(catalog, LimitServices::built_in())
    }

    pub fn with_limits(
        catalog: Arc<RwLock<ContainerCatalog>>,
        limit_services: LimitServices,
    ) -> Self {
        Self {
            inner: ClickHouseHttpHandler::with_limits(catalog, limit_services),
        }
    }

    pub fn with_tenancy(
        catalog: Arc<RwLock<ContainerCatalog>>,
        limit_services: LimitServices,
        tenancy: Arc<spacestorage_tenancy::Tenancy>,
    ) -> Self {
        Self {
            inner: ClickHouseHttpHandler::with_limits(catalog, limit_services).with_tenancy(tenancy),
        }
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
        Self {
            inner: ClickHouseHttpHandler::with_limits(catalog, limit_services)
                .with_tenancy(tenancy)
                .with_namespace_control(control),
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
