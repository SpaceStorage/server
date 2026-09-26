//! Admission: authz → tenancy admit → query admission (order documented in contracts).

use crate::error::Result;
use crate::quotas::QuotaSpec;
use crate::registry::NamespaceRecord;
use crate::usage::QuotaUsage;
use uuid::Uuid;

/// No-op when quotas empty (first binary). With `tenancy-quotas`, enforce specs.
pub fn admit(
    usage: &QuotaUsage,
    ns: &NamespaceRecord,
    consuming: &[(QuotaSpec, u64)],
) -> Result<()> {
    if ns.quotas.is_empty() {
        return Ok(());
    }
    #[cfg(not(feature = "tenancy-quotas"))]
    {
        let _ = (usage, consuming);
        // Quotas present without feature → should not happen if config gated;
        // treat as unlimited for first-binary compile.
        return Ok(());
    }
    #[cfg(feature = "tenancy-quotas")]
    {
        for (spec, amount) in consuming {
            usage.admit(ns.id, spec, *amount)?;
        }
        Ok(())
    }
}

pub fn release(usage: &QuotaUsage, ns_id: Uuid, specs: &[(QuotaSpec, u64)]) {
    for (spec, amount) in specs {
        usage.add(ns_id, spec.unit, &spec.datatype, -(*amount as i64));
    }
}
