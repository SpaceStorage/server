//! Tenancy errors (007 data-model validation codes).

use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TenancyError {
    #[error("NamespaceExists{{name={name}}}")]
    NamespaceExists { name: String },
    #[error("NamespaceNameInvalid{{name={name}}}")]
    NamespaceNameInvalid { name: String },
    #[error("NamespaceNotFound{{name={name}}}")]
    NamespaceNotFound { name: String },
    #[error("CascadeRequired{{name={name}, containers={containers:?}}}")]
    CascadeRequired { name: String, containers: Vec<Uuid> },
    #[error("NotClusterAdmin")]
    NotClusterAdmin,
    #[error("QuotaExceeded{{quota={quota}, limit={limit}, usage={usage}}}")]
    QuotaExceeded {
        quota: String,
        limit: u64,
        usage: u64,
    },
    #[error("QuotaUnitUnknown{{unit={unit}}}")]
    QuotaUnitUnknown { unit: String },
    #[error("QuotaNegative")]
    QuotaNegative,
    #[error("Slice7Required{{op={op}}}")]
    Slice7Required { op: String },
    #[error("ObservabilityRequired{{op={op}}}")]
    ObservabilityRequired { op: String },
    #[error("KeyMaterialForbidden")]
    KeyMaterialForbidden,
    #[error("EncryptionScopeInvalid{{mode={mode}, scope={scope}}}")]
    EncryptionScopeInvalid { mode: String, scope: String },
    #[error("AccessDenied{{verb={verb}, datatype={datatype}, namespace={namespace}}}")]
    AccessDenied {
        verb: String,
        datatype: String,
        namespace: String,
    },
    #[error("ReplicationNotTenant")]
    ReplicationNotTenant,
}

pub type Result<T> = std::result::Result<T, TenancyError>;
