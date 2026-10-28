//! `append_tenancy` namespace DDL shared by PostgreSQL, Cassandra, and ClickHouse.

use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use spacestorage_tenancy::NamespaceControl;
use spacestorage_types::ContainerCatalog;

pub struct ControlPlaneNamespaces {
    pub controlplane:
        Arc<tokio::sync::RwLock<Option<Arc<spacestorage_controlplane::ControlPlane>>>>,
    pub tenancy: Arc<spacestorage_tenancy::Tenancy>,
    pub catalog: Arc<RwLock<ContainerCatalog>>,
}

#[async_trait]
impl NamespaceControl for ControlPlaneNamespaces {
    async fn exists(&self, name: &str) -> Result<bool, String> {
        let guard = self.controlplane.read().await;
        let Some(cp) = guard.as_ref() else {
            return Ok(true);
        };
        Ok(cp.tenancy_namespace_by_name(name).is_some())
    }

    async fn create(&self, name: &str) -> Result<(), String> {
        let cp = self
            .controlplane
            .read()
            .await
            .clone()
            .ok_or_else(|| "controlplane not started".to_string())?;
        cp.append_tenancy(spacestorage_tenancy::TenancyOp::NamespaceCreate {
            name: name.to_string(),
        })
        .await
        .map_err(|error| error.to_string())?;
        self.sync(&cp);
        Ok(())
    }

    async fn drop_namespace(&self, name: &str) -> Result<(), String> {
        let cp = self
            .controlplane
            .read()
            .await
            .clone()
            .ok_or_else(|| "controlplane not started".to_string())?;
        let record = cp
            .tenancy_namespace_by_name(name)
            .ok_or_else(|| format!("NamespaceNotFound{{name={name}}}"))?;
        if let Some(message) = crate::namespace_catalog::namespace_delete_cascade_required(
            name,
            false,
            self.catalog.as_ref(),
        ) {
            return Err(message);
        }
        cp.append_tenancy(spacestorage_tenancy::TenancyOp::NamespaceDelete {
            id: record.id,
            cascade: false,
        })
        .await
        .map_err(|error| error.to_string())?;
        self.sync(&cp);
        Ok(())
    }

    async fn rename(&self, name: &str, new_name: &str) -> Result<(), String> {
        let cp = self
            .controlplane
            .read()
            .await
            .clone()
            .ok_or_else(|| "controlplane not started".to_string())?;
        let record = cp
            .tenancy_namespace_by_name(name)
            .ok_or_else(|| format!("NamespaceNotFound{{name={name}}}"))?;
        cp.append_tenancy(spacestorage_tenancy::TenancyOp::NamespaceRename {
            id: record.id,
            new_name: new_name.to_string(),
        })
        .await
        .map_err(|error| error.to_string())?;
        self.sync(&cp);
        Ok(())
    }
}

impl ControlPlaneNamespaces {
    fn sync(&self, cp: &spacestorage_controlplane::ControlPlane) {
        self.tenancy
            .registry
            .lock()
            .replace_applied(cp.tenancy_namespaces());
    }
}
