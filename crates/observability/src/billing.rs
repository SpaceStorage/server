//! Metrics → billing money formula (intent 16). Invoicing stays out of repo.
//!
//! Consumes `008` billing series (`namespace_usage_bytes` / `objects` / `connections`)
//! and applies operator-configured unit rates to produce a charge estimate.

use serde::{Deserialize, Serialize};

/// Per-unit rates in micro-currency (1e-6 of the billing currency).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BillingRates {
    /// Micro-currency per byte-month of `namespace_usage_bytes`.
    pub per_byte_month: u64,
    /// Micro-currency per object-month of `namespace_usage_objects`.
    pub per_object_month: u64,
    /// Micro-currency per connection-hour of `namespace_connections`.
    pub per_connection_hour: u64,
}

impl Default for BillingRates {
    fn default() -> Self {
        Self {
            // $0.023 / GB-month ≈ 23_000 micro / (1024^3) ≈ 0.000021 micro/byte — use round numbers for tests.
            per_byte_month: 1,
            per_object_month: 10,
            per_connection_hour: 100,
        }
    }
}

/// Snapshotted usage inputs (from Prometheus scrape / registry).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct UsageSnapshot {
    pub namespace: String,
    pub datatype: String,
    pub usage_bytes: u64,
    pub usage_objects: u64,
    pub connections: u64,
    /// Fraction of a month for bytes/objects (1.0 = full month).
    pub month_fraction: u64,
    /// Connection-hours in the window (connections × hours, already multiplied).
    pub connection_hours: u64,
}

/// Result of applying [`BillingRates`] to usage — not an invoice.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChargeEstimate {
    pub namespace: String,
    pub datatype: String,
    /// Total micro-currency for the window.
    pub amount_micros: u64,
    pub bytes_component: u64,
    pub objects_component: u64,
    pub connections_component: u64,
}

/// Apply the in-repo money formula:
/// `amount = bytes * rate_b * month_frac + objects * rate_o * month_frac + conn_hours * rate_c`.
pub fn estimate_charge(rates: &BillingRates, usage: &UsageSnapshot) -> ChargeEstimate {
    let month = usage.month_fraction.max(1);
    let bytes_component = usage
        .usage_bytes
        .saturating_mul(rates.per_byte_month)
        .saturating_mul(month);
    let objects_component = usage
        .usage_objects
        .saturating_mul(rates.per_object_month)
        .saturating_mul(month);
    let connections_component = usage
        .connection_hours
        .saturating_mul(rates.per_connection_hour);
    ChargeEstimate {
        namespace: usage.namespace.clone(),
        datatype: usage.datatype.clone(),
        amount_micros: bytes_component
            .saturating_add(objects_component)
            .saturating_add(connections_component),
        bytes_component,
        objects_component,
        connections_component,
    }
}

/// Sum estimates across namespaces (still not invoicing).
pub fn estimate_total(rates: &BillingRates, usages: &[UsageSnapshot]) -> u64 {
    usages
        .iter()
        .map(|u| estimate_charge(rates, u).amount_micros)
        .fold(0u64, |a, b| a.saturating_add(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formula_applies_rates() {
        let rates = BillingRates {
            per_byte_month: 2,
            per_object_month: 3,
            per_connection_hour: 5,
        };
        let usage = UsageSnapshot {
            namespace: "acme".into(),
            datatype: "kv_store".into(),
            usage_bytes: 10,
            usage_objects: 4,
            connections: 2,
            month_fraction: 1,
            connection_hours: 2,
        };
        let c = estimate_charge(&rates, &usage);
        assert_eq!(c.bytes_component, 20);
        assert_eq!(c.objects_component, 12);
        assert_eq!(c.connections_component, 10);
        assert_eq!(c.amount_micros, 42);
    }
}
