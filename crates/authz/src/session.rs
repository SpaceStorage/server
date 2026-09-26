//! In-memory session and optional bearer token (014 stubs).

use uuid::Uuid;

use crate::error::AuthzError;
use crate::principal::PrincipalId;

#[derive(Debug, Clone)]
pub struct Session {
    pub principal_id: PrincipalId,
    pub namespace_id: Option<Uuid>,
    pub credential_generation: u64,
    pub protocol: String,
}

impl Session {
    /// Later-request re-check seam (full logic in US1 T030).
    pub fn recheck(
        &self,
        enabled: bool,
        current_generation: u64,
    ) -> Result<(), AuthzError> {
        if !enabled {
            return Err(AuthzError::PrincipalDisabled);
        }
        if current_generation != self.credential_generation {
            return Err(AuthzError::AuthGenerationMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct SessionToken {
    pub token_hash: [u8; 32],
    pub principal_id: PrincipalId,
    pub generation: u64,
}
