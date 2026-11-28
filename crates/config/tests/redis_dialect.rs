//! `017` T011 — `protocols.redis.max_block` parse + validation.

use spacestorage_config::{NodeConfig, ValidateOptions, parse_validate};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn parse(text: &str) -> Result<spacestorage_config::NodeConfig, Vec<spacestorage_config::ConfigError>> {
    parse_validate(
        text,
        Path::new("redis-dialect.conf"),
        &[],
        &["admin", "admin-http"],
        ValidateOptions::fixtures_offline(),
    )
    .map(|(cfg, _)| cfg)
}

/// Absolute path + text of one `017` dialect config fixture.
fn fixture(name: &str) -> (String, PathBuf) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../specs/017-redis-dialect-coverage/contracts/fixtures")
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    (text, path)
}

fn parse_fixture(
    name: &str,
) -> Result<(NodeConfig, PathBuf), (Vec<spacestorage_config::ConfigError>, PathBuf)> {
    let (text, path) = fixture(name);
    match parse_validate(
        &text,
        &path,
        &[],
        &["admin", "admin-http", "redis"],
        ValidateOptions::fixtures_offline(),
    ) {
        Ok((cfg, _)) => Ok((cfg, path)),
        Err(err) => Err((err, path)),
    }
}

const BASE: &str = "disable admin;\ndisable admin-http;\n";

#[test]
fn max_block_unset_is_none() {
    let cfg = parse(BASE).expect("base config validates");
    assert_eq!(cfg.protocols.redis.max_block, None);
}

#[test]
fn max_block_seconds_parsed() {
    let text = format!("{BASE}protocols {{\n  redis {{\n    max_block 300;\n  }}\n}}\n");
    let cfg = parse(&text).expect("max_block config validates");
    assert_eq!(cfg.protocols.redis.max_block, Some(Duration::from_secs(300)));
}

#[test]
fn negative_max_block_rejected_with_named_error() {
    let text = format!("{BASE}protocols {{\n  redis {{\n    max_block -1;\n  }}\n}}\n");
    let err = parse(&text).unwrap_err();
    assert!(
        err.iter().any(|e| e.code == "redis_max_block_negative"),
        "{err:?}"
    );
}

// --- `017` contract fixtures (T003 / T066) -----------------------------------
// Registered here so the dialect config fixtures are exercised by the config
// suite. `complete-product-redis.conf` caps blocking waits; the memory-mode /
// pub-sub fixture leaves `max_block` unset (stock indefinite timeout 0); the
// negative fixture must fail with the named error.

#[test]
fn complete_product_fixture_parses_with_max_block() {
    let (cfg, _path) = parse_fixture("complete-product-redis.conf")
        .unwrap_or_else(|(err, path)| panic!("{} should validate: {err:?}", path.display()));
    assert_eq!(
        cfg.protocols.redis.max_block,
        Some(Duration::from_secs(300)),
        "complete-product-redis.conf declares max_block 300"
    );
}

#[test]
fn memory_mode_pubsub_fixture_parses_with_max_block_unset() {
    let (cfg, _path) = parse_fixture("memory-mode-pubsub.conf")
        .unwrap_or_else(|(err, path)| panic!("{} should validate: {err:?}", path.display()));
    assert_eq!(
        cfg.protocols.redis.max_block, None,
        "memory-mode-pubsub.conf leaves max_block unset → stock indefinite timeout 0"
    );
}

#[test]
fn max_block_negative_fixture_rejected_with_named_error() {
    let err = match parse_fixture("max-block-negative.conf") {
        Ok((cfg, _)) => panic!(
            "max-block-negative.conf must be rejected, got {:?}",
            cfg.protocols.redis
        ),
        Err((err, _path)) => err,
    };
    assert!(
        err.iter().any(|e| e.code == "redis_max_block_negative"),
        "{err:?}"
    );
}
