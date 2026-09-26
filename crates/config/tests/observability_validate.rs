//! Contract fixture tests for metrics / observability config (008 T016).

use spacestorage_config::{parse_validate, ErrorCode, ValidateOptions};
use std::path::Path;

fn load(rel: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../specs/008-observability/contracts/fixtures")
        .join(rel);
    std::fs::read_to_string(&root).unwrap_or_else(|e| panic!("read {}: {e}", root.display()))
}

#[test]
fn metrics_block_validates_offline() {
    let text = load("metrics-block.conf");
    let opts = ValidateOptions::fixtures_offline();
    let (cfg, _) = parse_validate(
        &text,
        Path::new("metrics-block.conf"),
        &[],
        &["admin", "admin-http"],
        opts,
    )
    .expect("metrics-block.conf should validate");
    assert!(!cfg.metrics.slow_query_enabled);
    assert!(!cfg.metrics.audit_log);
}

#[test]
fn otel_on_first_binary_rejected() {
    let text = load("invalid/otel-on-first-binary.conf");
    let opts = ValidateOptions {
        reject_slice9_observability: true,
        ..ValidateOptions::fixtures_offline()
    };
    let err = parse_validate(
        &text,
        Path::new("otel-on-first-binary.conf"),
        &[],
        &[],
        opts,
    )
    .unwrap_err();
    assert!(
        err.iter()
            .any(|e| e.code == ErrorCode::ObservabilitySlice9Required.as_str()),
        "expected ObservabilitySlice9Required, got {err:?}"
    );
}

#[test]
fn kafka_empty_brokers_invalid() {
    let text = load("invalid/kafka-empty-brokers.conf");
    let opts = ValidateOptions::observability_catalog();
    let opts = ValidateOptions {
        check_secrets_readable: false,
        require_transport: false,
        require_cluster_ports: false,
        require_master_key: false,
        ..opts
    };
    let err = parse_validate(
        &text,
        Path::new("kafka-empty-brokers.conf"),
        &[],
        &[],
        opts,
    )
    .unwrap_err();
    assert!(
        err.iter()
            .any(|e| e.code == ErrorCode::SinkConfigInvalid.as_str()),
        "expected SinkConfigInvalid, got {err:?}"
    );
}
