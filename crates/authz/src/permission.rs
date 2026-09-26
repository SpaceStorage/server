//! Closed verb bitmask; CLUSTER_ADMIN implies every other verb (014 T004).

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

    pub fn contains(self, other: Verb) -> bool {
        if self.0 & Self::CLUSTER_ADMIN.0 != 0 {
            return true;
        }
        self.0 & other.0 == other.0
    }

    pub fn bits(self) -> u16 {
        self.0
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

/// Stub authorizer — full wiring is US2.
pub struct Authorizer;

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
}
