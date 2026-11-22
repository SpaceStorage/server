//! Namespaces shell UI strings (018 T044 / T045).

#[test]
fn shell_namespace_mutations_and_confirm() {
    let js = include_str!("../src/assets/shell.js");
    assert!(js.contains("namespace_create"));
    assert!(js.contains("namespace_delete"));
    assert!(js.contains("namespace_rename"));
    assert!(js.contains("quota_replace"));
    assert!(js.contains("confirm("));
    assert!(js.contains("at least"));
    assert!(js.contains("loadSection"));
    assert!(!js.contains("sessionStorage"));
}