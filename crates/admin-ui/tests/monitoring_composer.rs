//! Monitoring composer (018 T051).

use spacestorage_admin_ui::monitoring::compose_monitoring_panel;

#[test]
fn absent_series_in_unavailable_not_zero() {
    let names = vec!["spacestorage_api_requests_total".to_string()];
    let j = compose_monitoring_panel("namespace", Some("acme"), &[], &names);
    let unavailable = j.get("unavailable").and_then(|v| v.as_array()).unwrap();
    assert_eq!(unavailable.len(), 1);
    assert_eq!(unavailable[0].get("reason").and_then(|v| v.as_str()), Some("series absent"));
    let series = j.get("series").and_then(|v| v.as_array()).unwrap();
    assert!(series.is_empty());
}