//! G9 multi_active create refused (FR-024b / T099 / T105).

use spacestorage_types::{Catalog, L3Model, StorageModeChoice, TypeError};

#[test]
fn g9_multi_active_on_create_refused() {
    let mut catalog = Catalog::new();
    assert!(
        matches!(
            catalog.create(
                "default",
                "kv_ma",
                L3Model::KvStore,
                true,
                None,
                StorageModeChoice::Persistent,
            ),
            Err(TypeError::MultiActiveUnsupported)
        ),
        "G9: catalog create multi_active=on → MultiActiveUnsupported; default off"
    );
    // Same refusal for the other first-binary L3 models.
    for (name, model, schema) in [
        (
            "rel_ma",
            L3Model::RelationalTable,
            Some(spacestorage_types::ContainerSchema {
                fields: vec![spacestorage_types::Field {
                    name: "id".into(),
                    domain: spacestorage_types::ValueDomain::Uuid,
                    nullable: false,
                }],
            }),
        ),
        ("doc_ma", L3Model::DocumentStore, None),
    ] {
        assert!(matches!(
            catalog.create(
                "default",
                name,
                model,
                true,
                schema,
                StorageModeChoice::Persistent,
            ),
            Err(TypeError::MultiActiveUnsupported)
        ));
    }
    // Default (off) still succeeds.
    catalog
        .create(
            "default",
            "kv_ok",
            L3Model::KvStore,
            false,
            None,
            StorageModeChoice::Persistent,
        )
        .expect("multi_active default off must create");
}
