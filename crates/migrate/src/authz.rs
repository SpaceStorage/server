//! Authz helpers for migrate / transform / restore (010 R15 / FR-010).

use crate::error::MigrateError;
use spacestorage_authz::permission::Verb;

#[derive(Debug, Clone, Copy)]
pub struct AuthzContext {
    pub verbs: Verb,
    /// True when principal holds MIGRATE on both named namespaces.
    pub dual_migrate: bool,
}

impl AuthzContext {
    pub fn cluster_admin() -> Self {
        Self {
            verbs: Verb::CLUSTER_ADMIN,
            dual_migrate: true,
        }
    }

    /// Build from a caller principal: cluster-admin token, MIGRATE, or dual MIGRATE.
    pub fn from_principal(cluster_admin: bool, has_migrate: bool, dual_migrate: bool) -> Self {
        if cluster_admin {
            return Self::cluster_admin();
        }
        let mut verbs = Verb::from_bits(0);
        if has_migrate {
            verbs = verbs | Verb::MIGRATE;
        }
        Self {
            verbs,
            dual_migrate: dual_migrate && has_migrate,
        }
    }

    pub fn allow_node_migrate(&self) -> Result<(), MigrateError> {
        if self.verbs.contains(Verb::CLUSTER_ADMIN) || self.verbs.contains(Verb::MIGRATE) {
            Ok(())
        } else {
            Err(MigrateError::AuthzDenied)
        }
    }

    pub fn allow_same_ns_transform(&self) -> Result<(), MigrateError> {
        self.allow_node_migrate()
    }

    pub fn allow_cross_namespace(&self) -> Result<(), MigrateError> {
        if self.verbs.contains(Verb::CLUSTER_ADMIN) || self.dual_migrate {
            Ok(())
        } else {
            Err(MigrateError::AuthzDenied)
        }
    }
}
