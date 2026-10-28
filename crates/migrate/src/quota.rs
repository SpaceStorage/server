//! Quota checks at start and cutover (010 R11 / FR-011) via `07` tenancy admit.

use crate::error::MigrateError;
use spacestorage_tenancy::{NamespaceRecord, QuotaSpec, QuotaUnit, QuotaUsage, Tenancy, admit};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, Default)]
pub struct QuotaView {
    pub logical_usage: u64,
    pub soft_limit: Option<u64>,
}

impl QuotaView {
    pub fn check_fits(&self, additional: u64) -> Result<(), MigrateError> {
        let Some(limit) = self.soft_limit else {
            return Ok(());
        };
        if self.logical_usage.saturating_add(additional) > limit {
            return Err(MigrateError::QuotaExceeded);
        }
        Ok(())
    }

    /// Read destination namespace bytes quota + current usage from `07`.
    pub fn from_tenancy(tenancy: &Tenancy, namespace: &str) -> Self {
        let reg = tenancy.registry.lock();
        let Some(ns) = reg.get_by_name(namespace) else {
            return Self::default();
        };
        Self::from_record(&tenancy.usage, ns)
    }

    pub fn from_record(usage: &QuotaUsage, ns: &NamespaceRecord) -> Self {
        let Some(spec) = ns
            .quotas
            .iter()
            .find(|q| matches!(q.unit, QuotaUnit::Bytes))
        else {
            return Self::default();
        };
        Self {
            logical_usage: usage.get(ns.id, spec.unit, &spec.datatype),
            soft_limit: Some(spec.limit),
        }
    }
}

/// Logical size of a row map (key+value lengths).
pub fn measure_row_bytes(rows: &HashMap<Vec<u8>, Vec<u8>>) -> u64 {
    rows.iter().map(|(k, v)| (k.len() + v.len()) as u64).sum()
}

/// Prefer a live `07` usage read when the namespace has a bytes quota; otherwise keep
/// the caller-supplied view (unit tests inject limits without tenancy records).
pub fn refresh_quota_view(tenancy: &Tenancy, namespace: &str, fallback: QuotaView) -> QuotaView {
    let fresh = QuotaView::from_tenancy(tenancy, namespace);
    if fresh.soft_limit.is_some() {
        fresh
    } else if fallback.soft_limit.is_some() {
        fallback
    } else {
        fresh
    }
}

/// Ensure a namespace record exists (empty quotas = unlimited) and refuse when a
/// second logical copy of `additional_bytes` would exceed `07` bytes quota.
pub fn admit_dest_bytes(
    tenancy: &Tenancy,
    namespace: &str,
    additional_bytes: u64,
) -> Result<QuotaView, MigrateError> {
    let ns = {
        let mut reg = tenancy.registry.lock();
        if let Some(ns) = reg.get_by_name(namespace) {
            ns.clone()
        } else {
            // Quota ledger create-if-absent. The job driver has no caller verb
            // bits at this point, so the record is opened as CLUSTER_ADMIN.
            reg.create(spacestorage_authz::Verb::CLUSTER_ADMIN.bits(), namespace)
                .map_err(|e| MigrateError::Io(e.to_string()))?
        }
    };
    let view = QuotaView::from_record(&tenancy.usage, &ns);
    view.check_fits(additional_bytes)?;
    if !ns.quotas.is_empty() {
        let consuming: Vec<(QuotaSpec, u64)> = ns
            .quotas
            .iter()
            .filter(|q| matches!(q.unit, QuotaUnit::Bytes))
            .map(|s| (s.clone(), additional_bytes))
            .collect();
        if !consuming.is_empty() {
            admit(&tenancy.usage, &ns, &consuming).map_err(|_| MigrateError::QuotaExceeded)?;
        }
    }
    Ok(QuotaView::from_record(&tenancy.usage, &ns))
}

/// Re-read destination quota before cutover/swap (no reservation).
pub fn recheck_dest_bytes(
    tenancy: &Tenancy,
    namespace: &str,
    additional_bytes: u64,
) -> Result<QuotaView, MigrateError> {
    let view = QuotaView::from_tenancy(tenancy, namespace);
    view.check_fits(additional_bytes)?;
    Ok(view)
}
