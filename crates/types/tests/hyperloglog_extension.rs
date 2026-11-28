//! `017` US5 T058: the registered L0 `hyperloglog` extension type.
//!
//! Verifies the extension path (`003` FR-045–FR-049): the cardinality-sketch
//! type resolves at L0 with its operation set, standard-error bound, canonical
//! version and storage modes, the identical `add`/`count`/`merge`/`clear`
//! behaviour is reachable through the shared abstract interface, and no
//! baseline descriptor was reduced or replaced.

use spacestorage_types::{
    HyperLogLog, Kind, L0Catalog, L0_CREATABLE, L0_NOT_CREATABLE, Level, StorageModeChoice,
    TypeCatalog,
};

#[test]
fn hyperloglog_registered_at_l0_with_contract_fields() {
    let cat = TypeCatalog::complete_product();
    let d = cat
        .get("hyperloglog")
        .expect("hyperloglog resolvable through complete_product()");

    assert_eq!(d.name, "hyperloglog");
    assert_eq!(d.display_name, "HyperLogLog");
    assert_eq!(d.level, Level::L0);
    assert_eq!(d.kind, Kind::DataStructure);
    // Free schema with an optional `precision` (default 14), not a required schema.
    assert!(!d.schema_required);
    assert_eq!(d.operations, vec!["add", "count", "merge", "clear"]);
    assert_eq!(d.standard_error, Some("1.04/sqrt(2^precision)"));
    assert_eq!(d.canonical_version, 1);

    // Machine-name resolution also works (catalog `resolve`/`describe`).
    assert_eq!(
        cat.resolve("hyperloglog").map(|d| d.name.as_str()),
        Some("hyperloglog")
    );
    assert_eq!(cat.describe("hyperloglog").unwrap().level, Level::L0);
}

#[test]
fn hyperloglog_is_an_l0_creatable_data_structure() {
    // FR-003 enumerates the L0 data structures; `017` adds `hyperloglog` as the
    // 16th. Assert membership and the contract-count, not a code-internal split.
    assert!(L0_CREATABLE.contains(&"hyperloglog"));
    assert_eq!(L0_CREATABLE.len(), 16);
    assert!(!L0_NOT_CREATABLE.contains(&"hyperloglog"));
}

#[test]
fn hyperloglog_creatable_in_every_storage_mode() {
    for mode in [
        StorageModeChoice::Memory,
        StorageModeChoice::Persistent,
        StorageModeChoice::Hybrid,
    ] {
        let mut cat = L0Catalog::new();
        cat.create("ns", "hll", "hyperloglog", mode)
            .unwrap_or_else(|e| panic!("create hyperloglog in {mode:?}: {e}"));
    }
}

#[test]
fn shared_interface_add_count_merge_clear() {
    // The identical algorithm is reachable directly through the shared type.
    let mut a = HyperLogLog::new(14);
    for i in 0..5000 {
        a.add(format!("user:{i}").as_bytes());
    }
    let err_a = a.standard_error();
    assert!(
        (err_a - 1.04 / (2f64.powi(14)).sqrt()).abs() < 1e-12,
        "standard_error mismatch: {err_a}"
    );
    let est_a = a.count() as f64;
    assert!(
        (est_a - 5000.0).abs() <= 5000.0 * err_a * 3.0,
        "estimate {est_a} outside 3*sigma of 5000"
    );

    // Merge gives the union sketch.
    let mut b = HyperLogLog::new(14);
    for i in 3000..8000 {
        b.add(format!("user:{i}").as_bytes());
    }
    let mut union = a.clone();
    union.merge(&b);
    // |0..5000 ∪ 3000..8000| = 8000.
    assert!(
        (union.count() as f64 - 8000.0).abs() <= 8000.0 * err_a * 3.0,
        "union estimate {} outside 3*sigma of 8000",
        union.count()
    );

    // Clear resets to empty.
    union.clear();
    assert_eq!(union.count(), 0);

    // `from_registers` round-trips the canonical representation.
    let rebuilt = HyperLogLog::from_registers(a.precision(), a.registers().to_vec());
    assert_eq!(rebuilt.count(), a.count());
}

#[test]
fn baseline_descriptors_unchanged_by_the_extension() {
    let cat = TypeCatalog::complete_product();

    // Every baseline descriptor the first-binary catalog already registered is
    // still present with its original level/kind.
    assert_eq!(cat.get("kv_store").map(|d| d.level), Some(Level::L3));
    assert_eq!(cat.get("relational_table").map(|d| d.level), Some(Level::L3));
    assert_eq!(cat.get("document_store").map(|d| d.level), Some(Level::L3));
    assert_eq!(cat.get("object_collection").map(|d| d.level), Some(Level::L3));
    assert_eq!(cat.get("bitmap").map(|d| d.kind), Some(Kind::DataStructure));
    assert_eq!(cat.get("bloom_filter").map(|d| d.level), Some(Level::L0));
    assert_eq!(cat.get("wal").map(|d| d.kind), Some(Kind::StoragePrimitive));
    assert_eq!(cat.get("lsm_tree").map(|d| d.kind), Some(Kind::StorageLayout));
    assert_eq!(cat.get("union").map(|d| d.level), Some(Level::L4));

    // The extension replaced no baseline: every descriptor except
    // `hyperloglog` keeps baseline defaults (empty op set, no standard error,
    // canonical version 1).
    for name in L0_CREATABLE.iter().chain(L0_NOT_CREATABLE.iter()) {
        if *name == "hyperloglog" {
            continue;
        }
        let d = cat.get(name).unwrap_or_else(|| panic!("{name} missing"));
        assert!(d.operations.is_empty(), "{name} gained an op set");
        assert!(d.standard_error.is_none(), "{name} gained a standard error");
        assert_eq!(d.canonical_version, 1, "{name} canonical version changed");
    }
}
