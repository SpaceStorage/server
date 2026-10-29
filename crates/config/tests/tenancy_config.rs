use spacestorage_config::{ValidateOptions, load_file};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../specs/007-tenancy-security/contracts/fixtures")
        .join(name)
}

fn codes(name: &str) -> Vec<String> {
    load_file(
        &fixture(name),
        &[],
        &[],
        ValidateOptions::fixtures_offline(),
    )
    .unwrap_err()
    .into_iter()
    .map(|error| error.code)
    .collect()
}

#[test]
fn negative_quota_has_named_error() {
    assert!(
        codes("invalid/quota-negative.conf")
            .iter()
            .any(|c| c == "QuotaNegative")
    );
}

#[cfg(not(feature = "tenancy-quotas"))]
#[test]
fn first_binary_refuses_quota_blocks() {
    assert!(
        codes("invalid/quotas-on-first-binary.conf")
            .iter()
            .any(|c| c == "Slice7Required")
    );
}

#[cfg(feature = "tenancy-quotas")]
#[test]
fn quota_block_is_not_slice7_when_enabled() {
    match load_file(
        &fixture("invalid/quotas-on-first-binary.conf"),
        &[],
        &[],
        ValidateOptions::fixtures_offline(),
    ) {
        Ok(_) => {}
        Err(errors) => assert!(
            errors.iter().all(|error| error.code != "Slice7Required"),
            "{errors:?}"
        ),
    }
}

#[test]
fn inline_encryption_key_is_forbidden() {
    assert!(
        codes("invalid/encryption-key-inline.conf")
            .iter()
            .any(|c| c == "KeyMaterialForbidden")
    );
}

#[test]
fn starter_namespace_is_resolved() {
    let path = fixture("namespace-block.conf");
    let text = std::fs::read_to_string(&path).unwrap();
    let doc = spacestorage_config::parser::parse(&text, path.to_str().unwrap()).unwrap();
    let cfg = spacestorage_config::resolve::resolve(&doc, path.to_str().unwrap(), &[]).unwrap();
    assert_eq!(cfg.cluster.starter_namespace.as_deref(), Some("acme"));
}
