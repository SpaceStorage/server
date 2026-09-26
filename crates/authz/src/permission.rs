//! Closed verb bitmask; CLUSTER_ADMIN implies every other verb (014 T004).

use crate::error::AuthzError;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Verb(u16);

impl Verb {
    pub const CLUSTER_ADMIN: Verb = Verb(1 << 0);
    pub const NAMESPACE_ADMIN: Verb = Verb(1 << 1);
    pub const READ: Verb = Verb(1 << 2);
    pub const WRITE: Verb = Verb(1 << 3);
    pub const CREATE: Verb = Verb(1 << 4);
    pub const DROP: Verb = Verb(1 << 5);
    pub const CONFIGURE: Verb = Verb(1 << 6);
    pub const REPLICATE: Verb = Verb(1 << 7);
    pub const MIGRATE: Verb = Verb(1 << 8);
    pub const AUDIT_READ: Verb = Verb(1 << 9);
    pub const METRICS_READ: Verb = Verb(1 << 10);

    pub const ALL: Verb = Verb(
        Self::CLUSTER_ADMIN.0
            | Self::NAMESPACE_ADMIN.0
            | Self::READ.0
            | Self::WRITE.0
            | Self::CREATE.0
            | Self::DROP.0
            | Self::CONFIGURE.0
            | Self::REPLICATE.0
            | Self::MIGRATE.0
            | Self::AUDIT_READ.0
            | Self::METRICS_READ.0,
    );

    /// First-binary FR-017 implicit tenant grant on bound namespace.
    pub const IMPLICIT_TENANT: Verb = Verb(
        Self::READ.0 | Self::WRITE.0 | Self::CREATE.0 | Self::DROP.0 | Self::CONFIGURE.0,
    );

    pub fn contains(self, other: Verb) -> bool {
        if self.0 & Self::CLUSTER_ADMIN.0 != 0 {
            return true;
        }
        self.0 & other.0 == other.0
    }

    pub fn bits(self) -> u16 {
        self.0
    }

    pub fn from_bits(bits: u16) -> Self {
        Verb(bits)
    }
}

impl std::ops::BitOr for Verb {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Verb(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for Verb {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

pub struct Resource {
    pub namespace_id: Option<Uuid>,
    pub container_id: Option<Uuid>,
}

pub enum AuthzDecision {
    Allow,
    Deny,
}

/// Custom role (slice 7 / `authz-custom`).
#[derive(Debug, Clone)]
pub struct CustomRole {
    pub name: String,
    pub verbs: Verb,
    /// Empty = all containers in namespace.
    pub container_ids: Vec<Uuid>,
    pub namespace_id: Option<Uuid>,
}

pub fn role_put_custom(role: CustomRole) -> Result<CustomRole, AuthzError> {
    if role.name == "admin" || role.name == "replication" {
        return Err(AuthzError::RoleNameReserved);
    }
    #[cfg(not(feature = "authz-custom"))]
    {
        let _ = role;
        return Err(AuthzError::Slice7Required);
    }
    #[cfg(feature = "authz-custom")]
    {
        Ok(role)
    }
}

/// Authorizer — first-binary implicit grant + optional custom roles.
pub struct Authorizer {
    pub custom: Vec<CustomRole>,
}

impl Default for Authorizer {
    fn default() -> Self {
        Self { custom: Vec::new() }
    }
}

impl Authorizer {
    pub fn authorize(
        &self,
        grants: Verb,
        verb: Verb,
        _resource: &Resource,
    ) -> AuthzDecision {
        if grants.contains(verb) {
            AuthzDecision::Allow
        } else {
            AuthzDecision::Deny
        }
    }

    /// Effective grants for a namespace-bound non-admin principal.
    pub fn effective_tenant_grants(
        &self,
        principal_id: Uuid,
        namespace_id: Uuid,
        is_admin: bool,
        is_replication: bool,
    ) -> Verb {
        if is_admin {
            return Verb::CLUSTER_ADMIN;
        }
        if is_replication {
            return Verb::REPLICATE;
        }
        #[cfg(feature = "authz-custom")]
        {
            let mut bits = 0u16;
            let mut any = false;
            for r in &self.custom {
                if r.namespace_id == Some(namespace_id) || r.namespace_id.is_none() {
                    let _ = principal_id;
                    bits |= r.verbs.bits();
                    any = true;
                }
            }
            if any {
                return Verb::from_bits(bits);
            }
        }
        #[cfg(not(feature = "authz-custom"))]
        {
            let _ = (principal_id, namespace_id);
        }
        Verb::IMPLICIT_TENANT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cluster_admin_implies_all() {
        let g = Verb::CLUSTER_ADMIN;
        assert!(g.contains(Verb::READ));
        assert!(g.contains(Verb::AUDIT_READ));
        assert!(g.contains(Verb::WRITE | Verb::CREATE));
    }

    #[test]
    fn custom_role_gated() {
        let err = role_put_custom(CustomRole {
            name: "reader".into(),
            verbs: Verb::READ,
            container_ids: Vec::new(),
            namespace_id: Some(Uuid::nil()),
        })
        .unwrap_err();
        assert_eq!(err, AuthzError::Slice7Required);
    }
}
