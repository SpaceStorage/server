//! Authenticated session bind: tenant login + namespace connection quota (007).

use crate::{ConnectionQuotaHold, Result, Tenancy, TenancyError, admit_tenant_login};
use std::sync::Arc;

/// After credential verification: `admit_tenant_login` then optional `connections` quota.
pub fn bind_namespace_session(
    tenancy: Option<&Arc<Tenancy>>,
    login: &str,
    namespace: &str,
) -> Result<Option<ConnectionQuotaHold>> {
    admit_tenant_login(login, Some(namespace))?;
    match tenancy {
        Some(t) => t.admit_connection(namespace),
        None => Ok(None),
    }
}

/// Cassandra `USE`: bound principals stay in their namespace; cluster `admin` may switch.
pub fn refuse_keyspace_open(
    login: &str,
    bound_namespace: &str,
    requested_keyspace: &str,
) -> Result<()> {
    admit_tenant_login(login, Some(bound_namespace))?;
    if login == "admin" || bound_namespace == requested_keyspace {
        Ok(())
    } else {
        Err(TenancyError::AccessDenied {
            verb: "bind".into(),
            datatype: "keyspace".into(),
            namespace: requested_keyspace.into(),
        })
    }
}

/// Later CQL runs only against a name present in the cluster registry.
pub fn ensure_namespace_registered(tenancy: &Tenancy, name: &str) -> Result<()> {
    if tenancy.registry.lock().get_by_name(name).is_some() {
        Ok(())
    } else {
        Err(TenancyError::NamespaceNotFound { name: name.into() })
    }
}

/// Session bind (auth / hello): accept namespace once against the cluster registry.
pub fn accept_namespace_on_bind(tenancy: Option<&Arc<Tenancy>>, name: &str) -> Result<()> {
    match tenancy {
        Some(t) => ensure_namespace_registered(t, name),
        None => Ok(()),
    }
}

/// When tenancy is attached, mark the session accepted once if `name` is in the cluster registry.
/// Missing or unknown names are ignored (handshake must not fail on the handler default).
pub fn accept_registry_presented(
    tenancy: Option<&Arc<Tenancy>>,
    registry_accepted: &mut bool,
    name: &str,
) {
    if *registry_accepted || tenancy.is_none() {
        return;
    }
    let Some(t) = tenancy else {
        return;
    };
    if ensure_namespace_registered(t, name).is_ok() {
        *registry_accepted = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyspace_use_refuses_cross_namespace_for_bound_principal() {
        let err = refuse_keyspace_open("demo", "demo", "other").unwrap_err();
        assert!(matches!(err, TenancyError::AccessDenied { .. }));
        assert!(refuse_keyspace_open("admin", "demo", "other").is_ok());
        assert!(refuse_keyspace_open("demo", "demo", "demo").is_ok());
    }

    #[test]
    fn ensure_namespace_registered_checks_registry() {
        use spacestorage_authz::Verb;

        let tenancy = Tenancy::new();
        tenancy
            .registry
            .lock()
            .create(Verb::CLUSTER_ADMIN.bits(), "acme")
            .unwrap();
        ensure_namespace_registered(&tenancy, "acme").unwrap();
        assert!(matches!(
            ensure_namespace_registered(&tenancy, "missing"),
            Err(TenancyError::NamespaceNotFound { .. })
        ));
    }

    #[test]
    fn accept_registry_presented_only_when_registered() {
        use spacestorage_authz::Verb;
        use std::sync::Arc;

        let tenancy = Arc::new(Tenancy::new());
        tenancy
            .registry
            .lock()
            .create(Verb::CLUSTER_ADMIN.bits(), "acme")
            .unwrap();
        let mut accepted = false;
        accept_registry_presented(Some(&tenancy), &mut accepted, "missing");
        assert!(!accepted);
        accept_registry_presented(Some(&tenancy), &mut accepted, "acme");
        assert!(accepted);
        accept_registry_presented(Some(&tenancy), &mut accepted, "other");
        assert!(accepted);
    }
}
