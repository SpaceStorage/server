use spacestorage_config::{parse_validate, ValidateOptions};
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../specs/001-runtime-cli-api/contracts/fixtures")
}

#[test]
fn minimal_conf_validates_offline() {
    let path = fixtures_dir().join("minimal.conf");
    let text = std::fs::read_to_string(&path).unwrap();
    let handlers = ["admin", "admin-http"];
    let (cfg, _) = parse_validate(
        &text,
        &path,
        &[],
        &handlers,
        ValidateOptions::fixtures_offline(),
    )
    .expect("minimal.conf should validate");
    assert!(cfg.disable_admin);
    assert_eq!(cfg.entrypoints.len(), 1);
    assert_eq!(cfg.entrypoints[0].handler, "admin-http");
}

#[test]
fn node_conf_validates_with_cassandra_stub() {
    let path = fixtures_dir().join("node.conf");
    let text = std::fs::read_to_string(&path).unwrap();
    let handlers = ["admin", "admin-http", "cassandra"];
    let (cfg, _) = parse_validate(
        &text,
        &path,
        &[],
        &handlers,
        ValidateOptions::fixtures_offline(),
    )
    .expect("node.conf should validate offline");
    assert_eq!(cfg.node_name, "db-1");
}

#[test]
fn unknown_handler_rejected() {
    let path = fixtures_dir().join("invalid/unknown-handler.conf");
    let text = std::fs::read_to_string(&path).unwrap();
    let handlers = ["admin", "admin-http"];
    let err = parse_validate(
        &text,
        &path,
        &[],
        &handlers,
        ValidateOptions::fixtures_offline(),
    )
    .unwrap_err();
    assert!(err.iter().any(|e| e.code == "entrypoint_unknown_handler"));
}

#[test]
fn threads_zero_rejected() {
    let path = fixtures_dir().join("invalid/threads-zero.conf");
    let text = std::fs::read_to_string(&path).unwrap();
    let handlers = ["admin", "admin-http"];
    let err = parse_validate(
        &text,
        &path,
        &[],
        &handlers,
        ValidateOptions::fixtures_offline(),
    )
    .unwrap_err();
    assert!(err.iter().any(|e| e.code == "threads_out_of_range"));
}
