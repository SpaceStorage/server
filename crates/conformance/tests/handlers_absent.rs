//! G8 absent handlers (skeleton — profile inventory asserted; startup gate needs config/node).

use spacestorage_conformance::in_process_cluster_ready;
use spacestorage_release_profile::{HandlerBuildSet, ReleaseProfile};

#[test]
fn g8_cassandra_forbidden_in_first_binary_profile() {
    let set = HandlerBuildSet::for_profile(ReleaseProfile::FirstBinary);
    for h in [
        "cassandra",
        "elasticsearch",
        "clickhouse",
        "clickhouse-http",
        "s3",
        "webdav",
    ] {
        assert!(set.is_forbidden(h), "{h} must be forbidden in FirstBinary");
    }
    assert!(
        in_process_cluster_ready(),
        "G8: unknown-handler-cassandra.conf → entrypoint_unknown_handler with known list"
    );
}
