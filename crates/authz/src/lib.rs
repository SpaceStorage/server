//! AuthZ — principals, SCRAM, permissions, sessions, bootstrap, audit (014).
//! Master-key / envelope live in `spacestorage-crypto`, not here.

pub mod audit;
pub mod bootstrap;
pub mod error;
pub mod permission;
pub mod principal;
pub mod scram;
pub mod session;

pub use error::AuthzError;
pub use permission::{role_put_custom, Authorizer, CustomRole, Resource, Verb};
pub use principal::{LoginName, PrincipalId, PrincipalRecord};
pub use session::{Session, SessionToken, SessionTokenStore};

pub const SCRAM_ITERATIONS_DEFAULT: u32 = 16384;

/// First-binary FR-017: namespace-bound non-admin principals get implicit
/// `{READ,WRITE,CREATE,DROP,CONFIGURE}` on that namespace only — not a stored role.
/// (Master-key material is `spacestorage_crypto::MasterKey`.)
pub fn first_binary_implicit_tenant_verbs() -> Verb {
    Verb::IMPLICIT_TENANT
}
