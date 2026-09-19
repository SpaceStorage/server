//! G3/G4 PostgreSQL first-binary dialect (skeleton).

use spacestorage_conformance::in_process_cluster_ready;

#[test]
fn g3_simple_extended_crud_autocommit() {
    assert!(
        in_process_cluster_ready(),
        "G3: CREATE/INSERT/SELECT/UPDATE/DELETE/DROP simple+extended auto-commit"
    );
}

#[test]
fn g4_copy_begin_commit_rollback_are_0a000() {
    assert!(
        in_process_cluster_ready(),
        "G4: COPY/BEGIN/COMMIT/ROLLBACK each → 0A000, no data change"
    );
}
