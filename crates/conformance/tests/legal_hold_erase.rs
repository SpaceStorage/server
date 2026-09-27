//! Legal-hold / erase conformance + migrate gate (full v1 Track 2).

#![cfg(feature = "complete-product")]

use parking_lot::Mutex;
use spacestorage_migrate::erase_with_legal;
use spacestorage_storage::{EraseRequest, LegalHold, LegalHoldStore};
use std::sync::Arc;
use uuid::Uuid;

#[test]
fn hold_blocks_erase_via_migrate_seam() {
    let store = Arc::new(Mutex::new(LegalHoldStore::default()));
    let hid = Uuid::now_v7();
    store.lock().place_hold(LegalHold {
        id: hid,
        namespace: "acme".into(),
        container: "users".into(),
        key: Some("u1".into()),
        reason: "litigation".into(),
        created_at_ms: 1,
    });
    let req = EraseRequest {
        namespace: "acme".into(),
        container: "users".into(),
        key: "u1".into(),
        requested_at_ms: 2,
    };
    assert!(erase_with_legal(&store, req.clone()).is_err());
    store.lock().release_hold(hid).unwrap();
    let rec = erase_with_legal(&store, req).unwrap();
    assert_eq!(rec.tombstone_seq, 1);
}
