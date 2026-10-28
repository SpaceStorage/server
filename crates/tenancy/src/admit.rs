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
        let mut committed: Vec<(QuotaSpec, u64)> = Vec::new();
        for (spec, amount) in consuming {
            match usage.admit(ns.id, spec, *amount) {
                Ok(()) => committed.push((spec.clone(), *amount)),
                Err(err) => {
                    if !committed.is_empty() {
                        release(usage, ns.id, &committed);
                    }
                    return Err(err);
                }
            }
        }
        Ok(())
    }
}

pub fn release(usage: &QuotaUsage, ns_id: Uuid, specs: &[(QuotaSpec, u64)]) {
    for (spec, amount) in specs {
        usage.release_commit(ns_id, spec, *amount);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quotas::{QuotaSpec, QuotaUnit};
    use crate::registry::NamespaceRecord;
    use parking_lot::Mutex;
    use spacestorage_internode::QuotaDelta;
    use std::sync::Arc;
    use uuid::Uuid;

    fn ns_with_quotas(quotas: Vec<QuotaSpec>) -> NamespaceRecord {
        let mut rec = NamespaceRecord::new("acme").unwrap();
        rec.quotas = quotas;
        rec
    }

    #[cfg(feature = "tenancy-quotas")]
    #[test]
    fn partial_multi_unit_admit_rolls_back_local_and_delta() {
        let usage = QuotaUsage::new();
        let deltas = Arc::new(Mutex::new(Vec::<QuotaDelta>::new()));
        usage.set_delta_notify(Some(Arc::new({
            let deltas = Arc::clone(&deltas);
            move |d| deltas.lock().push(d)
        })));
        let ns = ns_with_quotas(vec![
            QuotaSpec {
                unit: QuotaUnit::Bytes,
                limit: 100,
                datatype: None,
            },
            QuotaSpec {
                unit: QuotaUnit::Objects,
                limit: 1,
                datatype: None,
            },
        ]);
        let err = admit(
            &usage,
            &ns,
            &[
                (ns.quotas[0].clone(), 10),
                (ns.quotas[1].clone(), 5),
            ],
        )
        .unwrap_err();
        assert!(matches!(err, crate::error::TenancyError::QuotaExceeded { .. }));
        assert_eq!(usage.get(ns.id, QuotaUnit::Bytes, &None), 0);
        assert_eq!(usage.get(ns.id, QuotaUnit::Objects, &None), 0);
        let published = deltas.lock();
        assert_eq!(published.len(), 2);
        assert_eq!(published[0].delta, 10);
        assert_eq!(published[1].delta, -10);
    }

    #[cfg(feature = "tenancy-quotas")]
    #[test]
    fn release_publishes_negative_delta() {
        let usage = QuotaUsage::new();
        let deltas = Arc::new(Mutex::new(Vec::<QuotaDelta>::new()));
        usage.set_delta_notify(Some(Arc::new({
            let deltas = Arc::clone(&deltas);
            move |d| deltas.lock().push(d)
        })));
        let ns_id = Uuid::from_u128(1);
        let spec = QuotaSpec {
            unit: QuotaUnit::Bytes,
            limit: 100,
            datatype: None,
        };
        usage.admit(ns_id, &spec, 7).unwrap();
        deltas.lock().clear();
        release(&usage, ns_id, &[(spec, 7)]);
        assert_eq!(usage.get(ns_id, QuotaUnit::Bytes, &None), 0);
        let published = deltas.lock();
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].delta, -7);
    }
}
