//! G1/G2/G6/G7/G10 first-binary cluster conformance (skeleton — fails until node boots).

use spacestorage_conformance::in_process_cluster_ready;
use spacestorage_release_profile::{HandlerBuildSet, ReleaseProfile};

#[test]
fn g1_one_node_starter_write_quorum_one() {
    let set = HandlerBuildSet::for_profile(ReleaseProfile::FirstBinary);
    assert!(set.is_required("postgresql"));
    assert!(set.is_required("redis"));
    assert!(
        in_process_cluster_ready(),
        "G1: boot first-binary-one-node.conf → ready, internode+replication bound, /metrics, write_quorum ONE (needs crates/node)"
    );
}

#[test]
fn g2_three_node_join_write_quorum_two() {
    assert!(
        in_process_cluster_ready(),
        "G2: bootstrap A + join B/C → membership identical, ladder [az], write_quorum TWO"
    );
}

#[test]
fn g6_g7_quorum_two_not_min_live_and_restore() {
    assert!(
        in_process_cluster_ready(),
        "G6/G7: kill one → TWO succeeds; kill second → TWO fails; restart → readable"
    );
}

#[test]
fn g10_drain_blocks_new_tenant_connections() {
    assert!(
        in_process_cluster_ready(),
        "G10: drain → no new tenant connections; in-flight bounded by drain timeout"
    );
}
