//! G8 absent handlers — profile inventory + validate unknown-handler fixtures.

use spacestorage_config::{ValidateOptions, parse_validate};
use spacestorage_conformance::first_binary_handler_names;
use spacestorage_conformance::{fixtures_dir, validate_fixture};
use spacestorage_release_profile::{HandlerBuildSet, ReleaseProfile};
use std::path::PathBuf;

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
}

#[test]
fn g8_unknown_handler_fixtures_entrypoint_unknown_handler() {
    // Cassandra fixture from contracts.
    let err = validate_fixture("invalid/unknown-handler-cassandra.conf").unwrap_err();
    assert_unknown_handler(&err, "cassandra");

    // Same shape for other complete-product handlers (inline rewrite of the fixture).
    let template =
        std::fs::read_to_string(fixtures_dir().join("invalid/unknown-handler-cassandra.conf"))
            .unwrap();
    for handler in ["elasticsearch", "clickhouse", "s3", "webdav"] {
        let text = template.replace("handler cassandra;", &format!("handler {handler};"));
        let path = PathBuf::from(format!("invalid/unknown-handler-{handler}.conf"));
        let err = parse_validate(
            &text,
            &path,
            &[],
            first_binary_handler_names(),
            ValidateOptions {
                check_secrets_readable: false,
                ..ValidateOptions::first_binary()
            },
        )
        .unwrap_err();
        assert_unknown_handler(&err, handler);
    }
}

fn assert_unknown_handler(err: &[spacestorage_config::ConfigError], handler: &str) {
    let hit = err.iter().find(|e| e.code == "entrypoint_unknown_handler");
    assert!(
        hit.is_some(),
        "G8: expected entrypoint_unknown_handler for {handler}, got {err:?}"
    );
    let msg = &hit.unwrap().message;
    assert!(
        msg.contains(handler),
        "G8: message must name handler={handler}: {msg}"
    );
    // Known list includes first-binary handlers (admin, admin-http, internode, postgresql, redis, replication).
    for known in [
        "admin",
        "admin-http",
        "internode",
        "postgresql",
        "redis",
        "replication",
    ] {
        assert!(
            msg.contains(known),
            "G8: known list must mention {known}: {msg}"
        );
    }
}
