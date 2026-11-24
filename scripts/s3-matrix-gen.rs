//! Emit `docs/s3-compatibility-matrix.{md,json}` from the capability table.
//! Suite B rows stay false until `tests/s3-interop/matrix/results.json` records a pass.
//!
//! `rustc scripts/s3-matrix-gen.rs -o target/s3-matrix-gen && ./target/s3-matrix-gen`

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let root = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().expect("cwd"));
    let src = root.join("specs/019-s3-protocol/contracts/capability-matrix.md");
    let text = fs::read_to_string(&src).unwrap_or_else(|e| panic!("read {src:?}: {e}"));
    let suite_b_pass = suite_b_ok(&root.join("tests/s3-interop/matrix/results.json"));
    let mut section = "";
    let mut rows = Vec::new();
    for line in text.lines() {
        if line.starts_with("## Supported") {
            section = "Supported and tested";
        } else if line.starts_with("## Unsupported") {
            section = "Unsupported by design";
        } else if line.starts_with("## Not yet") {
            section = "Not yet implemented";
        } else if line.starts_with("## ") {
            section = "";
        }
        if section.is_empty() || !line.starts_with("| ") || line.contains("---") || line.contains("Operation")
            || line.contains("Capability")
        {
            continue;
        }
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(|c| c.trim()).collect();
        if cells.len() < 2 || cells[0].is_empty() {
            continue;
        }
        let supported = section == "Supported and tested";
        rows.push((
            cells[0].to_string(),
            section.to_string(),
            cells.get(1).copied().unwrap_or("").to_string(),
            supported,
            supported && suite_b_pass,
        ));
    }
    let mut json = String::from("{\n  \"source\": \"specs/019-s3-protocol/contracts/capability-matrix.md\",\n  \"suite_b_executed\": ");
    json.push_str(if suite_b_pass { "true" } else { "false" });
    json.push_str(",\n  \"rows\": [\n");
    for (i, (name, status, notes, suite_a, suite_b)) in rows.iter().enumerate() {
        if i > 0 {
            json.push_str(",\n");
        }
        json.push_str(&format!(
            "    {{\"operation\": \"{}\", \"status\": \"{}\", \"notes\": \"{}\", \"suite_a\": {}, \"suite_b\": {}}}",
            escape(name),
            escape(status),
            escape(notes),
            suite_a,
            suite_b
        ));
    }
    json.push_str("\n  ]\n}\n");
    let mut md = String::from("# S3 compatibility matrix\n\nGenerated from `specs/019-s3-protocol/contracts/capability-matrix.md`.\n\n");
    md.push_str("Suite A marks are true for Supported rows when this file is regenerated after the dialect_s3 test binaries pass. Suite B is true only when `tests/s3-interop/matrix/results.json` has no failed row.\n\n");
    md.push_str("| Operation | Status | Suite A | Suite B |\n|---|---|---|---|\n");
    for (name, status, _, suite_a, suite_b) in &rows {
        md.push_str(&format!("| {name} | {status} | {suite_a} | {suite_b} |\n"));
    }
    let docs = root.join("docs");
    fs::create_dir_all(&docs).ok();
    fs::write(docs.join("s3-compatibility-matrix.json"), &json).unwrap();
    fs::write(docs.join("s3-compatibility-matrix.md"), &md).unwrap();
    let art = root.join("specs/019-s3-protocol/artifacts");
    fs::create_dir_all(&art).ok();
    fs::write(art.join("s3-compatibility-matrix.json"), &json).unwrap();
    fs::write(art.join("s3-compatibility-matrix.md"), &md).unwrap();
    eprintln!("wrote {} rows", rows.len());
}

fn suite_b_ok(path: &std::path::Path) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    if !text.contains("\"rows\"") {
        return false;
    }
    // Harness emits `{"rows":[{"id":...,"pass":true|false},...]}`.
    if text.contains("\"pass\":false") || text.contains("\"pass\": false") {
        return false;
    }
    text.contains("\"pass\":true") || text.contains("\"pass\": true")
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
