//! Prometheus text 0.0.4 encoder (omit unknown labels; never emit empty/sentinel).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::sample::{SampleKind, SeriesSnapshot};

const CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

pub fn prometheus_content_type() -> &'static str {
    CONTENT_TYPE
}

fn escape_label_value(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for c in v.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            other => out.push(other),
        }
    }
    out
}

fn write_labels(buf: &mut String, labels: &[(String, String)]) {
    if labels.is_empty() {
        return;
    }
    buf.push('{');
    for (i, (k, v)) in labels.iter().enumerate() {
        if i > 0 {
            buf.push(',');
        }
        let _ = write!(buf, "{k}=\"{}\"", escape_label_value(v));
    }
    buf.push('}');
}

fn labels_vec(s: &SeriesSnapshot) -> Vec<(String, String)> {
    s.labels
        .iter()
        .filter(|(_, v)| !v.is_empty() && *v != "unknown" && *v != "-")
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Encode a snapshot as Prometheus exposition format 0.0.4.
pub fn encode_prometheus(series: &[SeriesSnapshot]) -> String {
    let mut by_name: BTreeMap<&str, Vec<&SeriesSnapshot>> = BTreeMap::new();
    for s in series {
        by_name.entry(s.name.as_str()).or_default().push(s);
    }

    let mut out = String::new();
    let mut emitted_header: BTreeSet<&str> = BTreeSet::new();

    for (name, group) in by_name {
        let help = group.first().map(|s| s.help).unwrap_or("");
        let kind = group.first().map(|s| s.kind).unwrap_or(SampleKind::Gauge);
        if emitted_header.insert(name) {
            let _ = writeln!(out, "# HELP {name} {help}");
            let type_str = match kind {
                SampleKind::Counter => "counter",
                SampleKind::Gauge => "gauge",
                SampleKind::Histogram => "histogram",
            };
            let _ = writeln!(out, "# TYPE {name} {type_str}");
        }

        for s in group {
            match s.kind {
                SampleKind::Counter => {
                    let v = s.counter.unwrap_or(0);
                    let labels = labels_vec(s);
                    out.push_str(name);
                    write_labels(&mut out, &labels);
                    let _ = writeln!(out, " {v}");
                }
                SampleKind::Gauge => {
                    let v = s.gauge.unwrap_or(0.0);
                    let labels = labels_vec(s);
                    out.push_str(name);
                    write_labels(&mut out, &labels);
                    let _ = writeln!(out, " {v}");
                }
                SampleKind::Histogram => {
                    let bounds = s.hist_bounds.unwrap_or(&[]);
                    let counts = s.hist_counts.as_deref().unwrap_or(&[]);
                    let mut cumulative = 0u64;
                    let base_labels = labels_vec(s);
                    for (i, bound) in bounds.iter().enumerate() {
                        cumulative += counts.get(i).copied().unwrap_or(0);
                        let mut labels = base_labels.clone();
                        labels.push(("le".into(), format!("{bound}")));
                        labels.sort_by(|a, b| a.0.cmp(&b.0));
                        out.push_str(name);
                        out.push_str("_bucket");
                        write_labels(&mut out, &labels);
                        let _ = writeln!(out, " {cumulative}");
                    }
                    // +Inf
                    cumulative += counts.get(bounds.len()).copied().unwrap_or(0);
                    let mut labels = base_labels.clone();
                    labels.push(("le".into(), "+Inf".into()));
                    labels.sort_by(|a, b| a.0.cmp(&b.0));
                    out.push_str(name);
                    out.push_str("_bucket");
                    write_labels(&mut out, &labels);
                    let _ = writeln!(out, " {cumulative}");

                    out.push_str(name);
                    out.push_str("_sum");
                    write_labels(&mut out, &base_labels);
                    let _ = writeln!(out, " {}", s.hist_sum.unwrap_or(0.0));

                    out.push_str(name);
                    out.push_str("_count");
                    write_labels(&mut out, &base_labels);
                    let _ = writeln!(out, " {}", s.hist_count.unwrap_or(0));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::{LabelSet, SampleKind, SeriesSnapshot};

    #[test]
    fn omit_label_never_emits_empty_or_unknown() {
        let mut with_user = LabelSet::new();
        with_user.insert("kind", "get").unwrap();
        with_user.insert("user", "alice").unwrap();
        let mut without_user = LabelSet::new();
        without_user.insert("kind", "get").unwrap();

        let series = vec![
            SeriesSnapshot {
                name: "spacestorage_query_total".into(),
                labels: with_user,
                kind: SampleKind::Counter,
                help: "query totals",
                counter: Some(2),
                gauge: None,
                hist_bounds: None,
                hist_counts: None,
                hist_sum: None,
                hist_count: None,
            },
            SeriesSnapshot {
                name: "spacestorage_query_total".into(),
                labels: without_user,
                kind: SampleKind::Counter,
                help: "query totals",
                counter: Some(4),
                gauge: None,
                hist_bounds: None,
                hist_counts: None,
                hist_sum: None,
                hist_count: None,
            },
        ];
        let text = encode_prometheus(&series);
        assert!(text.contains("user=\"alice\""));
        assert!(!text.contains("user=\"\""));
        assert!(!text.contains("user=\"unknown\""));
        assert!(text.contains("# HELP spacestorage_query_total"));
        assert_eq!(text.matches("# TYPE spacestorage_query_total").count(), 1);
    }
}
