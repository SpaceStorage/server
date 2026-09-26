//! Tenant namespace filter for scrape / OTel (FR-016, SC-003).

use crate::sample::SeriesSnapshot;

/// Keep only series whose `namespace` label equals `name`.
/// Drops node-global series (no `namespace` label) and foreign namespaces.
pub fn filter_namespace(series: &[SeriesSnapshot], name: &str) -> Vec<SeriesSnapshot> {
    series
        .iter()
        .filter(|s| s.labels.get("namespace") == Some(name))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::{LabelSet, SampleKind, SeriesSnapshot};

    fn snap(name: &str, ns: Option<&str>) -> SeriesSnapshot {
        let mut labels = LabelSet::new();
        if let Some(n) = ns {
            labels.insert("namespace", n).unwrap();
        }
        SeriesSnapshot {
            name: name.into(),
            labels,
            kind: SampleKind::Counter,
            help: "",
            counter: Some(1),
            gauge: None,
            hist_bounds: None,
            hist_counts: None,
            hist_sum: None,
            hist_count: None,
        }
    }

    #[test]
    fn tenant_filter_isolates_namespace() {
        let series = vec![
            snap("spacestorage_query_total", Some("acme")),
            snap("spacestorage_query_total", Some("other")),
            snap("spacestorage_uptime_seconds", None),
        ];
        let filtered = filter_namespace(&series, "acme");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].labels.get("namespace"), Some("acme"));
        assert!(filtered.iter().all(|s| s.labels.get("namespace") == Some("acme")));
        assert!(filtered.iter().all(|s| s.labels.get("namespace") != Some("other")));
    }
}
