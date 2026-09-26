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
pub use permission::{Authorizer, Verb};
pub use principal::{LoginName, PrincipalId, PrincipalRecord};

pub const SCRAM_ITERATIONS_DEFAULT: u32 = 16384;

/// First-binary FR-017: namespace-bound non-admin principals get implicit
/// `{READ,WRITE,CREATE,DROP,CONFIGURE}` on that namespace only — not a stored role.
/// (Master-key material is `spacestorage_crypto::MasterKey`.)
pub fn first_binary_implicit_tenant_verbs() -> Verb {
    Verb::READ | Verb::WRITE | Verb::CREATE | Verb::DROP | Verb::CONFIGURE
}
