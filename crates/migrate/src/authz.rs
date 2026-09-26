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
