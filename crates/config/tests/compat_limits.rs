//! 015 limits / product_version fixture validation.

use spacestorage_config::{parse_validate, ValidateOptions};
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../specs/015-compatibility-and-limits/contracts/fixtures")
}

#[test]
fn limits_zero_rejected() {
    let path = fixtures_dir().join("invalid/limits-zero.conf");
    let text = std::fs::read_to_string(&path).unwrap();
    let err = parse_validate(
        &text,
        &path,
        &[],
        &["admin", "admin-http"],
        ValidateOptions::fixtures_offline(),
    )
    .unwrap_err();
    assert!(
        err.iter().any(|e| e.code == "limits_zero"),
        "{err:?}"
    );
}

#[test]
fn product_version_zero_rejected() {
    let path = fixtures_dir().join("invalid/product-version-zero.conf");
    let text = std::fs::read_to_string(&path).unwrap();
    let err = parse_validate(
        &text,
        &path,
        &[],
        &["admin", "admin-http"],
        ValidateOptions::fixtures_offline(),
    )
    .unwrap_err();
    assert!(
        err.iter().any(|e| e.code == "product_version_zero"),
        "{err:?}"
    );
}

#[test]
fn limits_raised_parses_kib_mib() {
    let path = fixtures_dir().join("limits-raised.conf");
    let text = std::fs::read_to_string(&path).unwrap();
    // Append disable admin so offline validate does not require admin handlers.
    let text = format!("{text}\ndisable admin;\ndisable admin-http;\n");
    let (cfg, _) = parse_validate(
        &text,
        &path,
        &[],
        &["admin", "admin-http"],
        ValidateOptions::fixtures_offline(),
    )
    .expect("limits-raised.conf");
    assert_eq!(cfg.limits.limits.max_key, 4 * 1024);
    assert_eq!(cfg.limits.limits.max_value, 32 * 1024 * 1024);
    assert_eq!(cfg.limits.provenance, spacestorage_compat::LimitProvenance::Configured);
    assert_eq!(cfg.query.max_concurrent_per_node, Some(1024));
    assert_eq!(cfg.query.spill, Some(true));
}
