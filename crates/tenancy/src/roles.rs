//! Builtin + custom roles (custom behind tenancy-quotas / authz-custom).

use crate::error::{Result, TenancyError};
use serde::{Deserialize, Serialize};
use spacestorage_authz::Verb;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

/// Validates a role row before it is appended to the cluster log (`TenancyOp::RolePut`).
pub fn prepare_role_put(verb_bits: u16, role: Role) -> Result<Role> {
    if role.name == "admin" || role.name == "replication" {
        crate::ops::require_cluster_admin(verb_bits)?;
        return Ok(role);
    }
    role_put(verb_bits, role)
}

pub fn prepare_role_binding_put(verb_bits: u16, binding: RoleBinding) -> Result<RoleBinding> {
    crate::ops::require_cluster_admin(verb_bits)?;
    Ok(binding)
}

pub fn prepare_role_binding_delete(verb_bits: u16, principal_id: Uuid) -> Result<Uuid> {
    crate::ops::require_cluster_admin(verb_bits)?;
    Ok(principal_id)
}

pub fn role_put(verb_bits: u16, role: Role) -> Result<Role> {
    crate::ops::require_cluster_admin(verb_bits)?;
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

/// Tenant protocols (PostgreSQL, Redis, and the other client dialects).
/// `replication` is internodes-only. A non-admin principal needs exactly one namespace binding.
/// An empty verb set may authenticate when bound and authorizes no later verb.
pub fn authorize_tenant_protocol(role_name: &str, binding: Option<&RoleBinding>) -> Result<()> {
    if role_name == "replication" {
        return Err(TenancyError::ReplicationNotTenant);
    }
    if role_name == "admin" {
        return Ok(());
    }
    match binding {
        Some(b) if b.namespace_id.is_some() => Ok(()),
        _ => Err(TenancyError::AccessDenied {
            verb: "bind".into(),
            datatype: "*".into(),
            namespace: "*".into(),
        }),
    }
}

/// Login admission for a tenant protocol (PostgreSQL, Redis).
/// `admin` may omit a namespace. `replication` is refused. Any other login needs a non-empty bound namespace.
/// Enforce role verbs and optional per-type access policies on a consuming request.
pub fn authorize_consuming(
    login: &str,
    role: &Role,
    record: &crate::registry::NamespaceRecord,
    datatype: &str,
    requested: u16,
) -> Result<()> {
    if login == "admin" {
        return Ok(());
    }
    use crate::policy::{AccessPolicy, evaluate};
    let policy = record.policies.iter().find_map(|raw| {
        let p: AccessPolicy = serde_json::from_value(raw.clone()).ok()?;
        if p.datatype == datatype {
            Some(p)
        } else {
            None
        }
    });
    if evaluate(policy.as_ref(), role.verbs, requested) {
        Ok(())
    } else {
        Err(TenancyError::AccessDenied {
            verb: format!("0x{requested:x}"),
            datatype: datatype.into(),
            namespace: record.name.clone(),
        })
    }
}

pub fn admit_tenant_login(login: &str, bound_namespace: Option<&str>) -> Result<()> {
    if login == "replication" {
        return authorize_tenant_protocol("replication", None);
    }
    if login == "admin" {
        return authorize_tenant_protocol("admin", None);
    }
    let binding = bound_namespace
        .filter(|ns| !ns.is_empty())
        .map(|ns| RoleBinding {
            principal_id: Uuid::from_u128(stable_id(login)),
            role_name: login.to_string(),
            namespace_id: Some(Uuid::from_u128(stable_id(ns))),
        });
    authorize_tenant_protocol(login, binding.as_ref())
}

fn stable_id(s: &str) -> u128 {
    let mut hash: u128 = 0;
    for b in s.bytes() {
        hash = hash.wrapping_mul(131).wrapping_add(u128::from(b));
    }
    if hash == 0 { 1 } else { hash }
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

    #[cfg(not(feature = "tenancy-quotas"))]
    #[test]
    fn custom_gated() {
        let err = role_put(
            Verb::CLUSTER_ADMIN.bits(),
            Role {
                name: "reader".into(),
                scope_kind: ScopeKind::Namespace,
                verbs: Verb::READ.bits(),
                container_ids: Vec::new(),
            },
        )
        .unwrap_err();
        assert!(matches!(err, TenancyError::Slice7Required { .. }));
    }

    #[test]
    fn namespace_admin_cannot_put_role() {
        let err = role_put(
            Verb::NAMESPACE_ADMIN.bits(),
            Role {
                name: "reader".into(),
                scope_kind: ScopeKind::Namespace,
                verbs: Verb::READ.bits(),
                container_ids: Vec::new(),
            },
        )
        .unwrap_err();
        assert!(matches!(err, TenancyError::NotClusterAdmin));
    }

    #[test]
    fn replication_is_not_a_tenant() {
        let err = authorize_tenant_protocol("replication", None).unwrap_err();
        assert!(matches!(err, TenancyError::ReplicationNotTenant));
    }

    #[test]
    fn unbound_non_admin_is_refused_and_empty_binding_may_authenticate() {
        let err = authorize_tenant_protocol("reader", None).unwrap_err();
        assert!(matches!(err, TenancyError::AccessDenied { .. }));
        let bound = RoleBinding {
            principal_id: Uuid::from_u128(1),
            role_name: "reader".into(),
            namespace_id: Some(Uuid::from_u128(2)),
        };
        assert!(authorize_tenant_protocol("reader", Some(&bound)).is_ok());
        assert!(authorize_tenant_protocol("admin", None).is_ok());
    }

    #[test]
    fn login_admission_on_tenant_protocols() {
        assert!(admit_tenant_login("admin", None).is_ok());
        assert!(admit_tenant_login("demo", Some("demo")).is_ok());
        assert!(matches!(
            admit_tenant_login("replication", Some("demo")),
            Err(TenancyError::ReplicationNotTenant)
        ));
        assert!(matches!(
            admit_tenant_login("reader", None),
            Err(TenancyError::AccessDenied { .. })
        ));
        assert!(matches!(
            admit_tenant_login("reader", Some("")),
            Err(TenancyError::AccessDenied { .. })
        ));
    }

    #[test]
    fn read_only_role_cannot_write_on_consuming_path() {
        use crate::policy::AccessPolicy;
        use crate::registry::NamespaceRecord;
        let role = Role {
            name: "reader".into(),
            scope_kind: ScopeKind::Namespace,
            verbs: Verb::READ.bits(),
            container_ids: Vec::new(),
        };
        let mut record = NamespaceRecord::new("acme").unwrap();
        record.policies = vec![
            serde_json::to_value(AccessPolicy {
                datatype: "log_stream".into(),
                allow_verbs: Verb::READ.bits(),
            })
            .unwrap(),
        ];
        let err = authorize_consuming("reader", &role, &record, "kv_store", Verb::WRITE.bits())
            .unwrap_err();
        assert!(matches!(err, TenancyError::AccessDenied { .. }));
        assert!(
            authorize_consuming("reader", &role, &record, "log_stream", Verb::READ.bits(),).is_ok()
        );
    }
}
