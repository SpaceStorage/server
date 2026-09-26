//! Config validate for ingest (009).

use spacestorage_config::{parse_validate, ErrorCode, ValidateOptions};

#[test]
fn kafka_empty_brokers() {
    let err = parse_validate(
        r#"
node { name n1; }
disable admin;
disable admin-http;
ingest kafka bad {
  namespace acme;
  container events;
  topic t;
  group g;
  plaintext;
}
"#,
        std::path::Path::new("t.conf"),
        &[],
        &["admin", "admin-http"],
        ValidateOptions {
            reject_slice11_ui_ingest: false,
            ..ValidateOptions::fixtures_offline()
        },
    )
    .unwrap_err();
    assert!(
        err.iter()
            .any(|e| e.code == ErrorCode::IngestKafkaNoBrokers.as_str()),
        "{err:?}"
    );
}

#[test]
fn first_binary_rejects_kafka() {
    let err = parse_validate(
        r#"
node { name n1; }
disable admin;
disable admin-http;
ingest kafka acme-events {
  namespace acme;
  container events;
  brokers "127.0.0.1:9092";
  topic logs;
  group g;
  plaintext;
}
"#,
        std::path::Path::new("t.conf"),
        &[],
        &["admin", "admin-http"],
        ValidateOptions {
            reject_slice11_ui_ingest: true,
            ..ValidateOptions::fixtures_offline()
        },
    )
    .unwrap_err();
    assert!(
        err.iter()
            .any(|e| e.code == ErrorCode::UiIngestSlice11Required.as_str()),
        "{err:?}"
    );
}
