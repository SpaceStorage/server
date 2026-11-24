//! FirstBinary must still forbid S3 handler (019 T008, FR-013).

use spacestorage_config::{ValidateOptions, parse_validate};
use spacestorage_conformance::first_binary_handler_names;
use spacestorage_release_profile::{HandlerBuildSet, ReleaseProfile};
use std::path::Path;

#[test]
fn first_binary_forbids_s3_handler() {
    let set = HandlerBuildSet::for_profile(ReleaseProfile::FirstBinary);
    assert!(set.is_forbidden("s3"), "s3 must stay forbidden in FirstBinary");
    assert!(!set.is_required("s3"));
}

#[test]
fn handlers_complete_requires_s3() {
    let set = HandlerBuildSet::for_profile(ReleaseProfile::HandlersComplete);
    assert!(set.is_required("s3"));
    assert!(!set.is_forbidden("s3"));
}

#[test]
fn first_binary_unknown_s3_entrypoint() {
    let conf = r#"
node { name n; }
cluster { bootstrap; }
entrypoint s3 { address 127.0.0.1; port 9000; handler s3; plaintext; }
"#;
    let err = parse_validate(
        conf,
        &Path::new("test.conf"),
        &[],
        first_binary_handler_names(),
        ValidateOptions::first_binary(),
    )
    .unwrap_err();
    assert!(
        err.iter().any(|e| e.code == "entrypoint_unknown_handler"),
        "{err:?}"
    );
}