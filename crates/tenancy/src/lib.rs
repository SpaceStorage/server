//! Tenancy — namespaces, quotas, roles, policies, encryption attach (007).

pub mod admit;
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
pub use registry::{NamespaceRecord, NamespaceRegistry};
pub use usage::QuotaUsage;

/// Handle bundling registry + usage ledger.
#[derive(Default)]
pub struct Tenancy {
    pub registry: parking_lot::Mutex<NamespaceRegistry>,
    pub usage: QuotaUsage,
}

impl Tenancy {
    pub fn new() -> Self {
        Self::default()
    }
}
