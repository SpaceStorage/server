//! Namespace shell narrowing (018 T031).

#[test]
fn namespace_path_refusal() {
    use spacestorage_admin_ui::shell::check_namespace_path;
    assert!(check_namespace_path("/ui/overview", "namespace", &["acme".into()]).is_ok());
    assert!(check_namespace_path("/ui/namespaces/acme", "namespace", &["acme".into()]).is_ok());
    assert!(check_namespace_path("/ui/namespaces/other", "namespace", &["acme".into()]).is_err());
    assert!(check_namespace_path("/ui/namespaces/other", "cluster", &[]).is_ok());
}

#[test]
fn section_scope_gate() {
    use spacestorage_admin_ui::shell::{section_allowed_for_scope, ShellSection};
    assert!(section_allowed_for_scope(ShellSection::Overview, "namespace"));
    assert!(!section_allowed_for_scope(ShellSection::Audit, "namespace"));
}