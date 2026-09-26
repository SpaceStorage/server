//! Bootstrap first CLUSTER_ADMIN from config (stub; US1 T022).

use crate::error::AuthzError;

/// Returns Ok when bootstrap mint should run; Err(BootstrapAdminRequired) when
/// config claims bootstrap without admin_login/password file.
pub fn require_bootstrap_admin(has_login: bool, has_password_file: bool) -> Result<(), AuthzError> {
    if has_login && has_password_file {
        Ok(())
    } else {
        Err(AuthzError::BootstrapAdminRequired)
    }
}
