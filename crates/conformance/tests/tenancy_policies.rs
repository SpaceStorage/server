//! `007` access policy + tenant protocol binding (slice 7 when enabled).

use spacestorage_tenancy::roles::{RoleBinding, authorize_tenant_protocol};
use uuid::Uuid;

#[test]
fn read_only_role_still_requires_namespace_binding_for_tenant_protocols() {
    let err = authorize_tenant_protocol("reader", None).unwrap_err();
    assert!(err.to_string().contains("AccessDenied"), "{err}");
    let bound = RoleBinding {
        principal_id: Uuid::from_u128(1),
        role_name: "reader".into(),
        namespace_id: Some(Uuid::from_u128(2)),
    };
    assert!(authorize_tenant_protocol("reader", Some(&bound)).is_ok());
}

#[test]
fn replication_is_not_a_tenant_protocol() {
    let err = authorize_tenant_protocol("replication", None).unwrap_err();
    assert!(err.to_string().contains("ReplicationNotTenant"), "{err}");
}
