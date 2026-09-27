//! Named CDC conformance (full v1 Track 2).

#![cfg(feature = "complete-product")]

use spacestorage_types::{CdcCatalog, CdcEvent, CdcOp};

#[test]
fn named_cdc_publish_and_offset() {
    let mut cat = CdcCatalog::default();
    cat.create("acme", "orders_cdc", "orders").unwrap();
    let s = cat.get_mut("acme", "orders_cdc").unwrap();
    let seq = s.publish(CdcEvent {
        seq: 0,
        namespace: "acme".into(),
        container: "orders".into(),
        key: "o1".into(),
        op: CdcOp::Insert,
        before: None,
        after: Some(b"{}".to_vec()),
        hlc_physical: 1,
        hlc_logical: 0,
    });
    assert_eq!(seq, 1);
    s.commit_offset("billing", 2);
    assert_eq!(s.consumer_offset("billing"), 2);
}
