//! G5/G5b Redis first-binary dialect (skeleton).

use spacestorage_conformance::in_process_cluster_ready;

#[test]
fn g5_kv_must_list() {
    assert!(
        in_process_cluster_ready(),
        "G5: AUTH/PING/GET/SET/DEL/EXISTS/SCAN/SELECT-noop/TTL on K/V Store"
    );
}

#[test]
fn g5b_off_list_verbs_error_never_silent() {
    assert!(
        in_process_cluster_ready(),
        "G5b: HGET/JSON.GET return Redis error, never success or no-op"
    );
}
