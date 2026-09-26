//! Access policies (slice 7 / tenancy-quotas).

use crate::error::{Result, TenancyError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessPolicy {
    pub datatype: String,
    pub allow_verbs: u16,
}

pub fn policy_replace(policies: Vec<AccessPolicy>) -> Result<Vec<AccessPolicy>> {
    #[cfg(not(feature = "tenancy-quotas"))]
    {
        let _ = policies;
        return Err(TenancyError::Slice7Required {
            op: "PolicyReplace".into(),
        });
    }
    #[cfg(feature = "tenancy-quotas")]
    {
        Ok(policies)
    }
}

/// Missing policy ⇒ role verbs on all types; matching policy MAY further deny.
pub fn evaluate(policy: Option<&AccessPolicy>, verb_bits: u16, requested: u16) -> bool {
    match policy {
        None => verb_bits & requested == requested,
        Some(p) => (verb_bits & p.allow_verbs & requested) == requested,
    }
}
