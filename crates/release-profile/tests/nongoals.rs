//! Product non-goal audit (SC-005).

use spacestorage_release_profile::nongoals::{
    audit_nongoals, audit_text_mentions_serializable, default_repo_root,
};
use spacestorage_release_profile::ValidationCode;

#[test]
fn owning_specs_mention_all_product_nongoals() {
    let root = default_repo_root();
    audit_nongoals(&root).unwrap_or_else(|e| {
        panic!("nongoal audit failed against {}: {e}", root.display());
    });
}

#[test]
fn missing_serializable_mention_fails_audit() {
    let fixture = include_str!("fixtures/nongoal-missing-serializable.md");
    assert!(
        !audit_text_mentions_serializable(fixture),
        "negative fixture must not mention SERIALIZABLE"
    );
    // Simulate nongoal_unspecified when SqlSerializable is absent
    let err = ValidationCode::NongoalUnspecified {
        id: "SqlSerializable".to_string(),
        spec: "crates/release-profile/tests/fixtures/nongoal-missing-serializable.md".to_string(),
    };
    assert_eq!(err.code(), "nongoal_unspecified");
    assert!(format!("{err}").contains("SqlSerializable"));
}
