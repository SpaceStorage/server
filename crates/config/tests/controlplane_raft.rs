//! Invalid raft / exclusive-data fixtures (006 T072) — exit validate with errors.

use spacestorage_config::{parse_validate, ValidateOptions};
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../specs/006-control-plane/contracts/fixtures")
}

fn load(rel: &str) -> String {
    let path = fixtures_dir().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn handlers() -> [&'static str; 2] {
    ["admin", "admin-http"]
}

#[test]
fn raft_heartbeat_zero_rejected() {
    let text = load("invalid/raft-heartbeat-zero.conf");
    let err = parse_validate(
        &text,
        &fixtures_dir().join("invalid/raft-heartbeat-zero.conf"),
        &[],
        &handlers(),
        ValidateOptions::fixtures_offline(),
    )
    .unwrap_err();
    assert!(
        err.iter().any(|e| e.code == "raft_timeout_invalid"),
        "expected raft_timeout_invalid, got {err:?}"
    );
}

#[test]
fn raft_election_timeout_le_heartbeat_rejected() {
    let text = load("invalid/raft-election-timeout-le-heartbeat.conf");
    let err = parse_validate(
        &text,
        &fixtures_dir().join("invalid/raft-election-timeout-le-heartbeat.conf"),
        &[],
        &handlers(),
        ValidateOptions::fixtures_offline(),
    )
    .unwrap_err();
    assert!(
        err.iter().any(|e| e.code == "raft_timeout_invalid"),
        "expected raft_timeout_invalid, got {err:?}"
    );
}

#[test]
fn exclusive_data_on_first_binary_rejected() {
    let text = load("invalid/exclusive-data-on-first-binary.conf");
    let err = parse_validate(
        &text,
        &fixtures_dir().join("invalid/exclusive-data-on-first-binary.conf"),
        &[],
        &handlers(),
        ValidateOptions::fixtures_offline(),
    )
    .unwrap_err();
    assert!(
        err.iter().any(|e| e.code == "Slice7Required"),
        "expected Slice7Required for exclusive-data, got {err:?}"
    );
}
