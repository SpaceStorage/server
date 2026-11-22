//! Navigation labels use SpaceStorage nouns (018 T024).

#[test]
fn nav_labels_present() {
    let sources = [
        include_str!("../src/pages.rs"),
        include_str!("../src/shell.rs"),
        include_str!("../src/assets/shell.js"),
    ];
    for label in [
        "Overview",
        "Namespaces",
        "Nodes",
        "Monitoring",
        "Jobs",
        "Audit",
        "Cluster map",
        "Browse",
    ] {
        assert!(sources.iter().any(|s| s.contains(label)), "missing {label}");
    }
}

#[test]
fn no_elasticsearch_product_nouns_in_primary_shell_copy() {
    let html = spacestorage_admin_ui::pages::shell_html(
        "/ui/overview",
        spacestorage_admin_ui::shell::ShellSection::Overview,
    );
    let shell_rs = include_str!("../src/shell.rs");
    let shell_js = include_str!("../src/assets/shell.js");
    for bad in [" ILM", "data stream", "Index ", "Document "] {
        assert!(!html.contains(bad), "shell html contains forbidden noun");
        assert!(!shell_rs.contains(bad));
        assert!(!shell_js.contains(bad));
    }
}