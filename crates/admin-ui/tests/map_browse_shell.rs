//! Shell-hosted map/browse static checks (018 T063 / T065).

#[test]
fn map_namespace_picker_and_scoped_fetch() {
    let js = include_str!("../src/assets/shell.js");
    assert!(js.contains("map-ns-pick"));
    assert!(js.contains("SS_SHELL_MAP_NS"));
    assert!(js.contains("/ui/map?namespace="));
    let map = include_str!("../src/assets/cluster-map.js");
    assert!(map.contains("/v1/cluster/map?namespace="));
    assert!(map.contains("SS_SHELL_MAP_NS"));
    assert!(map.contains("shellHosted"));
    assert!(map.contains(r#"getAttribute("data-path")"#));
}

#[test]
fn browse_honors_namespace_query() {
    let js = include_str!("../src/assets/console.js");
    assert!(js.contains("namespace"));
    assert!(js.contains("URLSearchParams"));
    assert!(js.contains("applyBrowseDeepLink"));
    let pages = include_str!("../src/pages.rs");
    assert!(pages.contains("browse-mount"));
    assert!(pages.contains("console_panels"));
}