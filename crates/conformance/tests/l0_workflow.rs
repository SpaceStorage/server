//! L0 creatable workflow conformance (full v1 Track 2).

#![cfg(feature = "complete-product")]

use spacestorage_types::{L0Catalog, StorageModeChoice, L0_CREATABLE};

#[test]
fn l0_hash_table_workflow_create_put_get() {
    let mut cat = L0Catalog::new();
    cat.create("acme", "ht", "hash_table", StorageModeChoice::Memory)
        .unwrap();
    let c = cat.get_mut("acme", "ht").unwrap();
    c.put("k", b"v".to_vec()).unwrap();
    assert_eq!(c.get("k").unwrap(), Some(b"v".to_vec()));
}

#[test]
fn l0_all_creatable_and_wal_refused() {
    let mut cat = L0Catalog::new();
    for (i, t) in L0_CREATABLE.iter().enumerate() {
        cat.create("ns", format!("c{i}"), t, StorageModeChoice::Memory)
            .unwrap();
    }
    assert!(cat
        .create("ns", "w", "wal", StorageModeChoice::Persistent)
        .is_err());
}
