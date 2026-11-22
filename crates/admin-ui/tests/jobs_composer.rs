//! Jobs list pagination (018 T097).

use spacestorage_admin_ui::jobs_view::{compose_jobs_list, JobRow};
use spacestorage_admin_ui::shell::PageParams;

#[test]
fn jobs_desc_cursor_advances_page() {
    let rows: Vec<JobRow> = (0..3)
        .map(|i| JobRow {
            id: format!("job-{i:02}"),
            kind: "rebalance".into(),
            target: "cluster".into(),
            status: "running".into(),
            paused: false,
            progress: serde_json::json!({}),
            failure: None,
        })
        .collect();
    let j = compose_jobs_list(
        &PageParams {
            limit: 2,
            cursor: None,
        },
        rows.clone(),
    );
    assert_eq!(j["jobs"].as_array().unwrap().len(), 2);
    assert_eq!(j["jobs"][0]["id"], "job-02");
    let cur = j.get("next_cursor").and_then(|c| c.as_str()).expect("next_cursor");
    let j2 = compose_jobs_list(
        &PageParams {
            limit: 2,
            cursor: Some(cur.into()),
        },
        rows,
    );
    assert_eq!(j2["jobs"].as_array().unwrap().len(), 1);
    assert_eq!(j2["jobs"][0]["id"], "job-00");
    assert!(j2.get("next_cursor").and_then(|c| c.as_str()).is_none());
}