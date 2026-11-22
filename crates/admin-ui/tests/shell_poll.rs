//! Shell poll contract static checks (018 T034) — fail until shell implements contract.

#[test]
fn shell_poll_contract_strings() {
    let js = include_str!("../src/assets/shell.js");
    assert!(js.contains("10000") || js.contains("POLL_MS = 10000"), "10s poll interval");
    assert!(js.contains("document.hidden"), "no poll while hidden");
    assert!(js.contains("30000") || js.contains("STALE_MS = 30000"), "30s stale");
    assert!(js.contains("overviewInflight") || js.contains("Inflight"), "single-flight guard");
    assert!(js.contains("pollPaused") || js.contains("setPollPaused"), "pause control");
    assert!(js.contains("function esc(s)"), "html escape helper");
    assert!(js.contains("staleTickTimer"), "stale re-eval while paused");
    assert!(js.contains("sectionAgeRoots"), "stale all panel roots");
    assert!(js.contains("/ui/map") && js.contains("SS_SHELL_MAP_RENDER"), "map shell poll");
}

#[test]
fn shell_pagination_and_read_errors() {
    let js = include_str!("../src/assets/shell.js");
    assert!(js.contains("data-next-cursor"));
    assert!(js.contains("withPageQuery"));
    assert!(js.contains("showSectionReadError"));
    assert!(js.contains("monitoring/namespaces/"));
    assert!(js.contains("network error"));
    assert!(js.contains("syncMapStatusHeading"));
}