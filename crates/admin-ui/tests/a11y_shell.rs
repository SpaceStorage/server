//! Shell HTML structure smoke (018 T068 — no layout harness).

#[test]
fn shell_document_landmarks_and_labeled_login() {
    let html = spacestorage_admin_ui::pages::shell_html("/ui/overview", spacestorage_admin_ui::shell::ShellSection::Overview);
    assert!(html.contains(r#"role="main""#));
    assert!(html.contains(r#"<label class="field""#));
    assert!(html.contains("Admin token"));
    assert!(html.contains("id=\"admin-tok\""));
    let css = include_str!("../src/assets/shell.css");
    assert!(css.contains(":focus-visible"));
}

#[test]
fn shell_layout_note() {
    // Human quickstart §10: keyboard pass and 768px / 200% zoom on loopback — no browser harness in CI.
    assert!(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/assets/shell.css").exists());
}