//! Cluster-log tenancy op shapes.

use crate::error::{Result, TenancyError};
use crate::quotas::QuotaSpec;
use crate::roles::{Role, RoleBinding};
use serde::{Deserialize, Serialize};
use spacestorage_authz::Verb;
use uuid::Uuid;

/// FR-014: namespace registry, quota, and policy mutations require `CLUSTER_ADMIN`.
/// `NAMESPACE_ADMIN` and `CONFIGURE` do not grant these writes.
pub fn require_cluster_admin(verb_bits: u16) -> Result<()> {
    if Verb::from_bits(verb_bits).contains(Verb::CLUSTER_ADMIN) {
        Ok(())
    } else {
        Err(TenancyError::NotClusterAdmin)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum TenancyOp {
    NamespaceCreate {
        name: String,
    },
    NamespaceRename {
        id: Uuid,
        new_name: String,
    },
    NamespaceDelete {
        id: Uuid,
        cascade: bool,
    },
    QuotaReplace {
        id: Uuid,
        quotas: Vec<QuotaSpec>,
    },
    PolicyReplace {
        id: Uuid,
        policies: Vec<serde_json::Value>,
    },
    RolePut {
        role: Role,
    },
    RoleBindingPut {
        binding: RoleBinding,
    },
    RoleBindingDelete {
        principal_id: Uuid,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cluster_admin_may_mutate_registry() {
        assert!(require_cluster_admin(Verb::CLUSTER_ADMIN.bits()).is_ok());
    }

    #[test]
    fn namespace_admin_and_configure_are_refused() {
        assert!(matches!(
            require_cluster_admin(Verb::NAMESPACE_ADMIN.bits()),
            Err(TenancyError::NotClusterAdmin)
        ));
        assert!(matches!(
            require_cluster_admin(Verb::CONFIGURE.bits()),
            Err(TenancyError::NotClusterAdmin)
        ));
        assert!(matches!(
            require_cluster_admin(0),
            Err(TenancyError::NotClusterAdmin)
        ));
    }
}
