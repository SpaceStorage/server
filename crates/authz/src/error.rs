//! Normative validation / authz error codes (014 T009).

use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AuthzError {
    #[error("bootstrap admin required")]
    BootstrapAdminRequired,
    #[error("join secret is not an admin credential")]
    JoinSecretNotAdmin,
    #[error("unbound credential")]
    UnboundCredential,
    #[error("replication identity is not a tenant login")]
    ReplicationNotTenant,
    #[error("login exists")]
    LoginExists,
    #[error("login not found")]
    LoginNotFound,
    #[error("builtin role immutable")]
    BuiltinRoleImmutable,
    #[error("role name reserved")]
    RoleNameReserved,
    #[error("slice 7 required")]
    Slice7Required,
    #[error("auth generation mismatch")]
    AuthGenerationMismatch,
    #[error("principal disabled")]
    PrincipalDisabled,
    #[error("master key required")]
    MasterKeyRequired,
    #[error("master key permissions")]
    MasterKeyPermissions,
    #[error("key unresolvable")]
    KeyUnresolvable,
    #[error("key material forbidden")]
    KeyMaterialForbidden,
    #[error("transport omitted")]
    TransportOmitted,
    #[error("admin token removed")]
    AdminTokenRemoved,
    #[error("users file removed")]
    UsersFileRemoved,
    #[error("keyring removed")]
    KeyringRemoved,
    #[error("data-key rewrite not first-binary")]
    DataKeyRewriteNotFirstBinary,
}

impl AuthzError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::BootstrapAdminRequired => "BootstrapAdminRequired",
            Self::JoinSecretNotAdmin => "JoinSecretNotAdmin",
            Self::UnboundCredential => "UnboundCredential",
            Self::ReplicationNotTenant => "ReplicationNotTenant",
            Self::LoginExists => "LoginExists",
            Self::LoginNotFound => "LoginNotFound",
            Self::BuiltinRoleImmutable => "BuiltinRoleImmutable",
            Self::RoleNameReserved => "RoleNameReserved",
            Self::Slice7Required => "Slice7Required",
            Self::AuthGenerationMismatch => "AuthGenerationMismatch",
            Self::PrincipalDisabled => "PrincipalDisabled",
            Self::MasterKeyRequired => "MasterKeyRequired",
            Self::MasterKeyPermissions => "MasterKeyPermissions",
            Self::KeyUnresolvable => "KeyUnresolvable",
            Self::KeyMaterialForbidden => "KeyMaterialForbidden",
            Self::TransportOmitted => "TransportOmitted",
            Self::AdminTokenRemoved => "AdminTokenRemoved",
            Self::UsersFileRemoved => "UsersFileRemoved",
            Self::KeyringRemoved => "KeyringRemoved",
            Self::DataKeyRewriteNotFirstBinary => "DataKeyRewriteNotFirstBinary",
        }
    }
}
