//! Guarded action copy in shell.js (018 T057 / T060).

#[test]
fn drain_dialog_copy_and_empty_ack() {
    let js = include_str!("../src/assets/shell.js");
    assert!(js.contains("drain-impact"));
    assert!(js.contains("No new tenant connections or placements"));
    assert!(js.contains(r#"id="drain-target" type="text" value="""#));
    assert!(js.contains("confirmStrong"));
    assert!(js.contains("'cluster'"));
}

#[test]
fn dismiss_before_mutation_skips_fetch() {
    let js = include_str!("../src/assets/shell.js");
    assert!(js.contains("confirmNormal"));
    assert!(js.contains("guardedMutation"));
    assert!(js.contains("unconfirmed"));
    assert!(js.contains("Type the node name"));
    assert!(js.contains("confirmStrong(msg, name)"));
    assert!(js.contains("may accept placements again"));
    assert!(js.contains("Cancel job"));
    assert!(js.contains("confirmNormal(msg)"));
    assert!(js.contains("config-reload"));
}

#[test]
fn refusal_message_without_optimistic_refresh() {
    let js = include_str!("../src/assets/shell.js");
    assert!(js.contains("guardedMutation"));
    assert!(js.contains("refused"));
    assert!(js.contains("if (out.ok) loadSection()"));
}