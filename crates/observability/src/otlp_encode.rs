//! Minimal OTLP metrics protobuf encoder (ExportMetricsServiceRequest).
//!
//! Hand-rolled wire encoding for the subset used by SpaceStorage scrapes —
//! avoids pulling the full OpenTelemetry SDK into first-binary builds.

use crate::sample::{SampleKind, SeriesSnapshot};

pub fn encode_otlp_metrics(series: &[SeriesSnapshot], node_id: &str) -> Vec<u8> {
    let metrics: Vec<Vec<u8>> = series.iter().map(encode_metric).collect();
    let scope_metrics = {
        let mut buf = Vec::new();
        // ScopeMetrics.metrics = field 2 (repeated Metric)
        for m in &metrics {
            write_len_delimited(&mut buf, 2, m);
        }
        buf
    };
    let resource = encode_resource(node_id);
    let mut resource_metrics = Vec::new();
    // ResourceMetrics.resource = 1
    write_len_delimited(&mut resource_metrics, 1, &resource);
    // ResourceMetrics.scope_metrics = 2
    write_len_delimited(&mut resource_metrics, 2, &scope_metrics);

    let mut export_req = Vec::new();
    // ExportMetricsServiceRequest.resource_metrics = 1
    write_len_delimited(&mut export_req, 1, &resource_metrics);
    export_req
}

fn encode_resource(node_id: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    for (k, v) in [
        ("service.name", "spacestorage"),
        ("service.instance.id", node_id),
        ("service.namespace", "spacestorage"),
    ] {
        write_len_delimited(&mut buf, 1, &encode_key_value(k, v));
    }
    buf
}

fn encode_key_value(key: &str, value: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    write_string(&mut buf, 1, key);
    write_len_delimited(&mut buf, 2, &encode_any_string(value));
    buf
}

fn encode_any_string(value: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    write_string(&mut buf, 1, value); // AnyValue.string_value = 1
    buf
}

fn encode_metric(s: &SeriesSnapshot) -> Vec<u8> {
    let mut buf = Vec::new();
    write_string(&mut buf, 1, &s.name);
    if !s.help.is_empty() {
        write_string(&mut buf, 3, s.help);
    }
    match s.kind {
        SampleKind::Gauge => {
            write_len_delimited(&mut buf, 5, &encode_gauge(s));
        }
        SampleKind::Counter => {
            write_len_delimited(&mut buf, 7, &encode_sum(s));
        }
        SampleKind::Histogram => {
            write_len_delimited(&mut buf, 9, &encode_histogram(s));
        }
    }
    buf
}

fn encode_gauge(s: &SeriesSnapshot) -> Vec<u8> {
    let mut buf = Vec::new();
    write_len_delimited(&mut buf, 1, &encode_number_dp(s, s.gauge.unwrap_or(0.0)));
    buf
}

fn encode_sum(s: &SeriesSnapshot) -> Vec<u8> {
    let mut buf = Vec::new();
    // Sum.data_points = 1
    write_len_delimited(
        &mut buf,
        1,
        &encode_number_dp(s, s.counter.unwrap_or(0) as f64),
    );
    // aggregation_temporality = 2 → CUMULATIVE (2)
    write_varint_field(&mut buf, 2, 2);
    // is_monotonic = 3 → true
    write_bool_field(&mut buf, 3, true);
    buf
}

fn encode_histogram(s: &SeriesSnapshot) -> Vec<u8> {
    let mut buf = Vec::new();
    write_len_delimited(&mut buf, 1, &encode_hist_dp(s));
    write_varint_field(&mut buf, 2, 2); // CUMULATIVE
    buf
}

fn encode_number_dp(s: &SeriesSnapshot, value: f64) -> Vec<u8> {
    let mut buf = Vec::new();
    for (k, v) in s.labels.iter() {
        if v.is_empty() || v == "unknown" || v == "-" {
            continue;
        }
        write_len_delimited(&mut buf, 7, &encode_key_value(k, v)); // attributes = 7
    }
    write_fixed64_field(&mut buf, 3, value.to_bits()); // as_double = 3
    buf
}

fn encode_hist_dp(s: &SeriesSnapshot) -> Vec<u8> {
    let mut buf = Vec::new();
    for (k, v) in s.labels.iter() {
        if v.is_empty() || v == "unknown" || v == "-" {
            continue;
        }
        write_len_delimited(&mut buf, 9, &encode_key_value(k, v)); // attributes = 9
    }
    let count = s.hist_count.unwrap_or(0);
    write_fixed64_field(&mut buf, 2, count); // count = 2 (fixed64)
    if let Some(sum) = s.hist_sum {
        write_fixed64_field(&mut buf, 3, sum.to_bits()); // sum = 3
    }
    let bounds = s.hist_bounds.unwrap_or(&[]);
    let counts = s.hist_counts.as_deref().unwrap_or(&[]);
    let mut cumulative = 0u64;
    // Explicit bucket counts (field 6) — cumulative per OTLP histogram.
    for (i, _) in bounds.iter().enumerate() {
        cumulative += counts.get(i).copied().unwrap_or(0);
        write_varint_field(&mut buf, 6, cumulative);
    }
    // +Inf bucket
    cumulative += counts.get(bounds.len()).copied().unwrap_or(0);
    write_varint_field(&mut buf, 6, cumulative);
    for b in bounds {
        write_fixed64_field(&mut buf, 7, b.to_bits()); // explicit_bounds = 7
    }
    buf
}

fn write_tag(buf: &mut Vec<u8>, field: u32, wire: u8) {
    write_varint(buf, u64::from((field << 3) | u32::from(wire)));
}

fn write_varint(buf: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        buf.push((v as u8) | 0x80);
        v >>= 7;
    }
    buf.push(v as u8);
}

fn write_varint_field(buf: &mut Vec<u8>, field: u32, v: u64) {
    write_tag(buf, field, 0);
    write_varint(buf, v);
}

fn write_bool_field(buf: &mut Vec<u8>, field: u32, v: bool) {
    write_varint_field(buf, field, u64::from(v));
}

fn write_fixed64_field(buf: &mut Vec<u8>, field: u32, bits: u64) {
    write_tag(buf, field, 1);
    buf.extend_from_slice(&bits.to_le_bytes());
}

fn write_string(buf: &mut Vec<u8>, field: u32, s: &str) {
    write_len_delimited(buf, field, s.as_bytes());
}

fn write_len_delimited(buf: &mut Vec<u8>, field: u32, data: &[u8]) {
    write_tag(buf, field, 2);
    write_varint(buf, data.len() as u64);
    buf.extend_from_slice(data);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::{LabelSet, SampleKind};

    #[test]
    fn encodes_non_empty_export_request() {
        let mut labels = LabelSet::new();
        let _ = labels.insert("kind", "query");
        let series = [SeriesSnapshot {
            name: "spacestorage_query_total".into(),
            labels,
            kind: SampleKind::Counter,
            help: "queries",
            counter: Some(3),
            gauge: None,
            hist_bounds: None,
            hist_counts: None,
            hist_sum: None,
            hist_count: None,
        }];
        let bytes = encode_otlp_metrics(&series, "node-1");
        assert!(bytes.len() > 16);
        // Field 1 len-delimited tag = (1<<3)|2 = 0x0a
        assert_eq!(bytes[0], 0x0a);
        assert!(bytes.windows(b"spacestorage".len()).any(|w| w == b"spacestorage"));
        assert!(bytes
            .windows(b"spacestorage_query_total".len())
            .any(|w| w == b"spacestorage_query_total"));
    }
}
