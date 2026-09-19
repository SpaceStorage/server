//! Ledger integration tests (SC-004).

use spacestorage_release_profile::{MilestoneRecord, ValidationCode};
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for _ in 0..6 {
        if dir.join("docs/milestones/001-first-binary.yaml").is_file() {
            return dir;
        }
        if !dir.pop() {
            break;
        }
    }
    panic!("could not locate repo root from {}", env!("CARGO_MANIFEST_DIR"));
}

#[test]
fn first_binary_milestone_requires_deferred_6_through_11() {
    let path = repo_root().join("docs/milestones/001-first-binary.yaml");
    let record = MilestoneRecord::load_file(&path).expect("load yaml");
    record.validate().expect("001-first-binary must validate");
    assert_eq!(record.implemented, vec![1, 2, 3, 4, 5]);
    let ids: Vec<u8> = record.deferred.iter().map(|d| d.id).collect();
    assert_eq!(ids, vec![6, 7, 8, 9, 10, 11]);
    assert!(record.deferred.iter().all(|d| d.still_owed));
}

#[test]
fn missing_deferred_fixture_fails() {
    let path = repo_root().join("docs/milestones/fixtures/missing-deferred.yaml");
    let record = MilestoneRecord::load_file(&path).expect("load fixture");
    let err = record.validate().expect_err("must fail deferred_missing");
    assert_eq!(err.code(), "deferred_missing");
    match err {
        ValidationCode::DeferredMissing { missing } => {
            assert!(missing.contains(&8), "expected slice 8 missing, got {missing:?}");
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn cancelled_still_owed_false_is_deferred_marked_cancelled() {
    let yaml = r#"
slug: cancelled
profile: first-binary
implemented: [1, 2, 3, 4, 5]
deferred:
  - { id: 6, still_owed: true, reason: "x" }
  - { id: 7, still_owed: true, reason: "x" }
  - { id: 8, still_owed: false, reason: "deleted intent" }
  - { id: 9, still_owed: true, reason: "x" }
  - { id: 10, still_owed: true, reason: "x" }
  - { id: 11, still_owed: true, reason: "x" }
changelog_ref: x.md
"#;
    let record = MilestoneRecord::parse_yaml(yaml).unwrap();
    assert_eq!(
        record.validate().unwrap_err().code(),
        "deferred_marked_cancelled"
    );
}

#[test]
fn holes_are_slice_gap() {
    let yaml = r#"
slug: hole
profile: first-binary
implemented: [1, 2, 4, 5]
deferred:
  - { id: 3, still_owed: true, reason: "x" }
  - { id: 6, still_owed: true, reason: "x" }
  - { id: 7, still_owed: true, reason: "x" }
  - { id: 8, still_owed: true, reason: "x" }
  - { id: 9, still_owed: true, reason: "x" }
  - { id: 10, still_owed: true, reason: "x" }
  - { id: 11, still_owed: true, reason: "x" }
changelog_ref: x.md
"#;
    let record = MilestoneRecord::parse_yaml(yaml).unwrap();
    assert_eq!(record.validate().unwrap_err().code(), "slice_gap");
}

#[test]
fn complete_product_missing_slices_profile_incomplete() {
    let yaml = r#"
slug: incomplete
profile: complete-product
implemented: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
deferred: []
changelog_ref: x.md
"#;
    let record = MilestoneRecord::parse_yaml(yaml).unwrap();
    assert_eq!(record.validate().unwrap_err().code(), "profile_incomplete");
}
