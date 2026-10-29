//! Access policies (slice 7 / tenancy-quotas).

use crate::error::Result;
#[cfg(not(feature = "tenancy-quotas"))]
use crate::error::TenancyError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessPolicy {
    pub datatype: String,
    pub allow_verbs: u16,
}

pub fn policy_replace(verb_bits: u16, policies: Vec<AccessPolicy>) -> Result<Vec<AccessPolicy>> {
    crate::ops::require_cluster_admin(verb_bits)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use spacestorage_authz::Verb;

    #[test]
    fn read_only_cannot_write_and_type_policy_can_deny_one_datatype() {
        let read = Verb::READ.bits();
        let write = Verb::WRITE.bits();
        assert!(evaluate(None, read, read));
        assert!(!evaluate(None, read, write));
        assert!(!evaluate(None, 0, read));
        let kv_only = AccessPolicy {
            datatype: "kv_store".into(),
            allow_verbs: Verb::READ.bits() | Verb::WRITE.bits(),
        };
        assert!(evaluate(Some(&kv_only), read | write, write));
        let log_read = AccessPolicy {
            datatype: "log_stream".into(),
            allow_verbs: Verb::READ.bits(),
        };
        assert!(!evaluate(Some(&log_read), read | write, write));
        assert!(evaluate(Some(&log_read), read | write, read));
    }

    #[test]
    fn configure_cannot_replace_policy() {
        let err = policy_replace(Verb::CONFIGURE.bits(), Vec::new()).unwrap_err();
        assert!(matches!(err, TenancyError::NotClusterAdmin));
    }
}
