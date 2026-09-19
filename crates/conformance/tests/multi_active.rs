//! G9 multi_active create refused (skeleton).

use spacestorage_conformance::in_process_cluster_ready;

#[test]
fn g9_multi_active_on_create_refused() {
    assert!(
        in_process_cluster_ready(),
        "G9: catalog create multi_active=on → multi_active_unsupported; default off"
    );
}
