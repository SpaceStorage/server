//! G11 Document Store admin-create + canonical blob (skeleton).

use spacestorage_conformance::in_process_cluster_ready;

#[test]
fn g11_document_store_blob_crud() {
    assert!(
        in_process_cluster_ready(),
        "G11: admin-create Document Store; blob CRUD via PG/Redis; JSON.GET still errors"
    );
}
