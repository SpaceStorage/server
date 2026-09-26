//! G11 Document Store admin-create + canonical blob via Redis.

use spacestorage_conformance::boot_one_node;
use spacestorage_handler_redis::{RedisReply, SessionState, dispatch};
use spacestorage_types::{L3Model, StorageModeChoice};
use std::sync::Arc;
use tempfile::tempdir;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn g11_document_store_blob_crud() {
    let dir = tempdir().unwrap();
    let lab = boot_one_node(dir.path()).await;

    // Admin-create persistent Document Store on the live node catalog.
    {
        let mut cat = lab.node.catalog.write().expect("catalog");
        cat.create(
            "demo",
            "docs",
            L3Model::DocumentStore,
            false,
            None,
            StorageModeChoice::Persistent,
        )
        .expect("G11: admin-create Document Store");
        assert_eq!(
            cat.describe("demo", "docs").expect("describe").model,
            L3Model::DocumentStore
        );
    }

    // Canonical blob CRUD via Redis dialect (GET/SET) — not native document verbs.
    let mut s = SessionState::demo(Arc::clone(&lab.node.catalog));
    s.container = "docs".into();
    assert!(matches!(
        dispatch(&mut s, "AUTH", &["demo", "demo"]),
        RedisReply::Ok
    ));
    assert!(matches!(
        dispatch(&mut s, "SET", &["doc1", r#"{"hello":"world"}"#]),
        RedisReply::Ok
    ));
    match dispatch(&mut s, "GET", &["doc1"]) {
        RedisReply::Bulk(Some(v)) => assert_eq!(v, br#"{"hello":"world"}"#),
        other => panic!("G11: expected blob GET, got {other:?}"),
    }
    assert!(matches!(
        dispatch(&mut s, "DEL", &["doc1"]),
        RedisReply::Integer(1)
    ));
    // JSON.GET still errors on that container (SC-006 / native verbs not required).
    assert!(
        matches!(
            dispatch(&mut s, "JSON.GET", &["doc1"]),
            RedisReply::Error(_)
        ),
        "G11: JSON.GET must error on Document Store"
    );

    lab.shutdown().await;
}
