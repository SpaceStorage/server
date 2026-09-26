//! Cluster-log tenancy op shapes.

use crate::quotas::QuotaSpec;
use crate::roles::{Role, RoleBinding};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum TenancyOp {
    NamespaceCreate { name: String },
    NamespaceRename { id: Uuid, new_name: String },
    NamespaceDelete { id: Uuid, cascade: bool },
    QuotaReplace { id: Uuid, quotas: Vec<QuotaSpec> },
    PolicyReplace { id: Uuid, policies: Vec<serde_json::Value> },
    RolePut { role: Role },
    RoleBindingPut { binding: RoleBinding },
    RoleBindingDelete { principal_id: Uuid },
}
