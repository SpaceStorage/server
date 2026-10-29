//! Unknown handler names are rejected. Every protocol the server speaks is known.

use spacestorage_config::{ValidateOptions, parse_validate};
use spacestorage_conformance::fixtures_dir;
use spacestorage_node::HANDLER_NAMES;
use std::path::PathBuf;

#[test]
fn server_handlers_include_every_protocol() {
    for h in [
        "postgresql",
        "redis",
        "cassandra",
        "elasticsearch",
        "clickhouse",
        "clickhouse-http",
        "s3",
        "webdav",
        "syslog",
    ] {
        assert!(HANDLER_NAMES.contains(&h), "{h} is part of the server");
    }
}

#[test]
fn unknown_handler_name_is_rejected() {
    let template =
        std::fs::read_to_string(fixtures_dir().join("invalid/unknown-handler-cassandra.conf"))
            .unwrap();
    let text = template.replace("handler cassandra;", "handler telnet;");
    let path = PathBuf::from("invalid/unknown-handler-telnet.conf");
    let err = parse_validate(
        &text,
        &path,
        &[],
        HANDLER_NAMES,
        ValidateOptions {
            check_secrets_readable: false,
            ..ValidateOptions::complete_product()
        },
    )
    .unwrap_err();
    let hit = err.iter().find(|e| e.code == "entrypoint_unknown_handler");
    assert!(
        hit.is_some(),
        "expected entrypoint_unknown_handler, got {err:?}"
    );
    assert!(hit.unwrap().message.contains("telnet"));
}
