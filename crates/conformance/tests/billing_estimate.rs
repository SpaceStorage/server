//! Billing money formula conformance (full v1 Track 2) — no invoicing.

#![cfg(feature = "complete-product")]

use spacestorage_observability::{estimate_charge, BillingRates, UsageSnapshot};

#[test]
fn billing_estimate_from_usage_snapshot() {
    let rates = BillingRates {
        per_byte_month: 2,
        per_object_month: 5,
        per_connection_hour: 10,
    };
    let usage = UsageSnapshot {
        namespace: "acme".into(),
        datatype: "kv_store".into(),
        usage_bytes: 100,
        usage_objects: 3,
        connections: 0,
        month_fraction: 1,
        connection_hours: 2,
    };
    let est = estimate_charge(&rates, &usage);
    assert_eq!(est.bytes_component, 200);
    assert_eq!(est.objects_component, 15);
    assert_eq!(est.connections_component, 20);
    assert_eq!(est.amount_micros, 235);
}
