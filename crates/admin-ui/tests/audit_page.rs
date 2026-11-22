//! Audit page composer shape (018 T056 fixture).

use spacestorage_admin_ui::audit::merge_audit_page;
use spacestorage_admin_ui::shell::PageParams;

#[test]
fn audit_response_matches_fixture_top_level_keys() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../specs/018-admin-ui-modernization/contracts/fixtures/audit-page.json"
    ))
    .unwrap();
    let page = merge_audit_page(
        &PageParams {
            limit: 50,
            cursor: None,
        },
        50,
        None,
        vec![],
    );
    for key in ["generated_at", "limit", "next_cursor", "entries"] {
        assert!(page.get(key).is_some(), "missing {key}");
        assert!(fixture.get(key).is_some(), "fixture missing {key}");
    }
}