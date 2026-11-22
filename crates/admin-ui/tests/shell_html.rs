//! Unauthenticated shell HTML gate (018 T023).

#[test]
fn overview_html_has_login_without_tenant_payload() {
    let html = spacestorage_admin_ui::pages::shell_html(
        "/ui/overview",
        spacestorage_admin_ui::shell::ShellSection::Overview,
    );
    assert!(html.contains("login-panel"));
    assert!(html.contains("data-path=\"/ui/overview\""));
    assert!(html.contains("<nav"));
    assert!(!html.to_lowercase().contains("namespace acme"));
    assert!(!html.contains("/v1/ui/"));
}