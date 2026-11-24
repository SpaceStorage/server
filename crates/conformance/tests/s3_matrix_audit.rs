//! Supported matrix rows must record suite_a after generation (019 T081).

#[test]
fn supported_rows_have_suite_a() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/s3-compatibility-matrix.json"
    );
    let text = std::fs::read_to_string(path).expect(path);
    let value: serde_json::Value = serde_json::from_str(&text).expect("json");
    let rows = value["rows"].as_array().expect("rows");
    assert!(!rows.is_empty());
    for row in rows {
        if row["status"] == "Supported and tested" {
            assert_eq!(
                row["suite_a"], true,
                "missing suite_a for {}",
                row["operation"]
            );
        }
    }
}
