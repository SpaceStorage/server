//! Builtin + custom roles (custom behind tenancy-quotas / authz-custom).

use crate::error::{Result, TenancyError};
use serde::{Deserialize, Serialize};
use spacestorage_authz::Verb;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    pub name: String,
    pub scope_kind: ScopeKind,
    pub verbs: u16,
    pub container_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    Cluster,
    Namespace,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleBinding {
    pub principal_id: Uuid,
    pub role_name: String,
    pub namespace_id: Option<Uuid>,
}

pub fn builtin_admin() -> Role {
    Role {
        name: "admin".into(),
        scope_kind: ScopeKind::Cluster,
        verbs: Verb::CLUSTER_ADMIN.bits(),
        container_ids: Vec::new(),
    }
}

pub fn builtin_replication() -> Role {
    Role {
        name: "replication".into(),
        scope_kind: ScopeKind::Cluster,
        verbs: Verb::REPLICATE.bits(),
        container_ids: Vec::new(),
    }
}

pub fn role_put(role: Role) -> Result<Role> {
    if role.name == "admin" || role.name == "replication" {
        return Err(TenancyError::from_authz_reserved());
    }
    #[cfg(not(feature = "tenancy-quotas"))]
    {
        let _ = role;
        return Err(TenancyError::Slice7Required {
            op: "RolePut".into(),
        });
    }
    #[cfg(feature = "tenancy-quotas")]
    {
        Ok(role)
    }
}

impl TenancyError {
    fn from_authz_reserved() -> Self {
        // Map to AccessDenied-ish; 014 owns BuiltinRoleImmutable — surface as Slice7/deny.
        Self::AccessDenied {
            verb: "RolePut".into(),
            datatype: "*".into(),
            namespace: "*".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins() {
        assert_eq!(builtin_admin().name, "admin");
        assert_eq!(builtin_replication().verbs, Verb::REPLICATE.bits());
    }

    #[test]
    fn custom_gated() {
        let err = role_put(Role {
            name: "reader".into(),
            scope_kind: ScopeKind::Namespace,
            verbs: Verb::READ.bits(),
            container_ids: Vec::new(),
        })
        .unwrap_err();
        assert!(matches!(err, TenancyError::Slice7Required { .. }));
    }
}
