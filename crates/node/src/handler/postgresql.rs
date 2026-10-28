//! Node adapter for `spacestorage-handler-postgresql`.

use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use spacestorage_compat::LimitServices;
use spacestorage_handler_postgresql::{PostgresqlHandler, UserStore};
use spacestorage_types::ContainerCatalog;
use tokio_util::sync::CancellationToken;

use crate::handler::{ClientStream, Handler};
use crate::handler::controlplane_namespaces::ControlPlaneNamespaces;

pub struct PostgresqlPortHandler {
    inner: PostgresqlHandler,
}

impl PostgresqlPortHandler {
    pub fn new(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self::with_release_limits(catalog, builtin_release(), LimitServices::built_in())
    }

    pub fn with_release(catalog: Arc<RwLock<ContainerCatalog>>, release: &str) -> Self {
        Self::with_release_limits(catalog, release, LimitServices::built_in())
    }

    pub fn with_release_limits(
        catalog: Arc<RwLock<ContainerCatalog>>,
        release: &str,
        limit_services: LimitServices,
    ) -> Self {
        Self {
            inner: PostgresqlHandler::with_release_limits(
                catalog,
                Arc::new(UserStore::demo()),
                release,
                limit_services,
            ),
        }
    }

    pub fn with_namespace_control(
        catalog: Arc<RwLock<ContainerCatalog>>,
        release: &str,
        limit_services: LimitServices,
        controlplane: Arc<
            tokio::sync::RwLock<Option<Arc<spacestorage_controlplane::ControlPlane>>>,
        >,
        tenancy: Arc<spacestorage_tenancy::Tenancy>,
    ) -> Self {
        Self {
            inner: PostgresqlHandler::with_release_limits(
                Arc::clone(&catalog),
                Arc::new(UserStore::demo()),
                release,
                limit_services,
            )
            .with_namespace_control(Arc::new(ControlPlaneNamespaces {
                controlplane,
                tenancy: Arc::clone(&tenancy),
                catalog,
            }))
            .with_tenancy(tenancy),
        }
    }

    pub fn with_users(catalog: Arc<RwLock<ContainerCatalog>>, users: Arc<UserStore>) -> Self {
        Self {
            inner: PostgresqlHandler::with_release(catalog, users, builtin_release()),
        }
    }
}

/// Dialect name for this node's Cargo features (`016` release profile).
fn builtin_release() -> &'static str {
    if cfg!(feature = "complete-product") {
        "complete-product"
    } else if cfg!(feature = "handlers-complete") {
        "handlers-complete"
    } else {
        "first-binary"
    }
}

#[async_trait]
impl Handler for PostgresqlPortHandler {
    fn name(&self) -> &str {
        "postgresql"
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

#[cfg(test)]
mod tests {
    use super::*;
    use spacestorage_tenancy::{QuotaSpec, QuotaUnit, Tenancy};

    #[test]
    fn port_handler_uses_build_release_profile() {
        let cat = Arc::new(RwLock::new(spacestorage_types::ContainerCatalog::new()));
        let h = PostgresqlPortHandler::new(cat);
        assert_eq!(h.inner.profile.as_str(), builtin_release());
    }

    #[cfg(feature = "tenancy-quotas")]
    #[test]
    fn session_connect_admits_connections_quota_unit() {
        let tenancy = std::sync::Arc::new(Tenancy::new());
        let namespace = tenancy
            .registry
            .lock()
            .create(spacestorage_authz::Verb::CLUSTER_ADMIN.bits(), "acme")
            .unwrap();
        tenancy
            .registry
            .lock()
            .replace_quotas(
                spacestorage_authz::Verb::CLUSTER_ADMIN.bits(),
                namespace.id,
                vec![QuotaSpec {
                    unit: QuotaUnit::Connections,
                    limit: 1,
                    datatype: None,
                }],
            )
            .unwrap();
        let hold = tenancy.admit_connection("acme").unwrap();
        assert!(hold.is_some());
        assert!(tenancy.admit_connection("acme").is_err());
    }
}
