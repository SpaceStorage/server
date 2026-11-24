//! `protocols.s3` parse tests (019 T009).

use spacestorage_config::resolve::resolve;

fn parse_ok(text: &str) -> spacestorage_config::NodeConfig {
    let doc = spacestorage_config::parser::parse(text, "test.conf").expect("parse");
    resolve(&doc, "test.conf", &[]).expect("resolve")
}

#[test]
fn s3_protocol_defaults() {
    let cfg = parse_ok(
        "node { name test; }\ncluster { bootstrap; }\nentrypoint s3 { address 127.0.0.1; port 9000; handler s3; plaintext; }\n",
    );
    assert_eq!(cfg.protocols.s3.region, "us-east-1");
    assert_eq!(cfg.protocols.s3.virtual_host_suffix, None);
    assert_eq!(cfg.protocols.s3.multipart_max_parts, 10_000);
    assert_eq!(cfg.protocols.s3.multipart_min_part, 5 * 1024 * 1024);
    assert_eq!(
        cfg.protocols.s3.multipart_idle,
        std::time::Duration::from_secs(24 * 3600)
    );
}

#[test]
fn s3_protocol_directives() {
    let cfg = parse_ok(
        r#"
node { name test; }
cluster { bootstrap; }
protocols { s3 { region eu-west-1; virtual_host_suffix s3.local; multipart_idle 1h; multipart_min_part 5MiB; multipart_max_parts 500; } }
entrypoint s3 { address 127.0.0.1; port 9000; handler s3; plaintext; }
"#,
    );
    assert_eq!(cfg.protocols.s3.region, "eu-west-1");
    assert_eq!(
        cfg.protocols.s3.virtual_host_suffix.as_deref(),
        Some("s3.local")
    );
    assert_eq!(cfg.protocols.s3.multipart_max_parts, 500);
    assert_eq!(cfg.protocols.s3.multipart_min_part, 5 * 1024 * 1024);
    assert_eq!(
        cfg.protocols.s3.multipart_idle,
        std::time::Duration::from_secs(3600)
    );
}