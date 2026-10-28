//! Tenancy — namespaces, quotas, roles, policies, encryption attach (007).

pub mod admit;
pub mod namespace_control;
pub mod session;
pub mod encryption;
pub mod error;
pub mod name;
pub mod ops;
pub mod policy;
pub mod quotas;
pub mod registry;
pub mod roles;
pub mod usage;

pub use admit::admit;
pub use error::{Result, TenancyError};
pub use ops::TenancyOp;
pub use quotas::{QuotaSpec, QuotaUnit};
pub use namespace_control::{NamespaceControl, PermissiveNamespaceControl};
pub use registry::{NamespaceRecord, NamespaceRegistry};
pub use roles::{admit_tenant_login, authorize_consuming, Role};
pub use session::bind_namespace_session;
pub use usage::QuotaUsage;
use std::collections::BTreeMap;

/// Handle bundling registry + usage ledger.
#[derive(Default)]
pub struct Tenancy {
    pub registry: parking_lot::Mutex<NamespaceRegistry>,
    pub usage: std::sync::Arc<QuotaUsage>,
    pub roles: parking_lot::Mutex<BTreeMap<String, Role>>,
}

impl Tenancy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn admit_connection(
        self: &std::sync::Arc<Self>,
        namespace: &str,
    ) -> Result<Option<ConnectionQuotaHold>> {
        let record = self.registry.lock().get_by_name(namespace).cloned();
        let Some(record) = record else {
            return Ok(None);
        };
        let specs: Vec<_> = record
            .quotas
            .iter()
            .filter(|spec| spec.unit == QuotaUnit::Connections)
            .cloned()
            .map(|spec| (spec, 1))
            .collect();
        admit(&self.usage, &record, &specs)?;
        Ok(Some(ConnectionQuotaHold {
            tenancy: std::sync::Arc::clone(self),
            namespace_id: record.id,
            specs,
        }))
    }
}

pub struct ConnectionQuotaHold {
    tenancy: std::sync::Arc<Tenancy>,
    namespace_id: uuid::Uuid,
    specs: Vec<(QuotaSpec, u64)>,
}

impl Drop for ConnectionQuotaHold {
    fn drop(&mut self) {
        admit::release(&self.tenancy.usage, self.namespace_id, &self.specs);
    }
}
