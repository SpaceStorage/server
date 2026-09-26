//! Sample types, label sets, and process-wide Registry (008).

use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::ObservabilityError;

/// Prometheus metric name: `^[a-zA-Z_:][a-zA-Z0-9_:]*$`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MetricName(String);

impl MetricName {
    pub fn new(name: impl Into<String>) -> Result<Self, ObservabilityError> {
        let name = name.into();
        if !is_valid_metric_name(&name) {
            return Err(ObservabilityError::UnknownLabelForbidden {
                detail: format!("invalid metric name '{name}'"),
            });
        }
        Ok(Self(name))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for MetricName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

fn is_valid_metric_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == ':' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
}

fn is_valid_label_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Label set: key absent when unknown (FR-023). Empty / sentinels forbidden.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LabelSet(BTreeMap<String, String>);

impl LabelSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(
        &mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<(), ObservabilityError> {
        let name = name.into();
        let value = value.into();
        if !is_valid_label_name(&name) {
            return Err(ObservabilityError::UnknownLabelForbidden {
                detail: format!("invalid label name '{name}'"),
            });
        }
        if value.is_empty() || value == "unknown" || value == "-" {
            return Err(ObservabilityError::UnknownLabelForbidden {
                detail: format!("forbidden label value for '{name}'"),
            });
        }
        self.0.insert(name, value);
        Ok(())
    }

    /// Insert only when `value` is `Some` and non-sentinel.
    pub fn insert_known(
        &mut self,
        name: impl Into<String>,
        value: Option<impl Into<String>>,
    ) -> Result<(), ObservabilityError> {
        if let Some(v) = value {
            self.insert(name, v)?;
        }
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(|s| s.as_str())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn inner(&self) -> &BTreeMap<String, String> {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleKind {
    Counter,
    Gauge,
    Histogram,
}

#[derive(Debug)]
pub struct HistogramState {
    pub bounds: &'static [f64],
    pub counts: Vec<AtomicU64>,
    pub sum: AtomicU64,   // bits of f64
    pub count: AtomicU64,
}

impl HistogramState {
    pub fn new(bounds: &'static [f64]) -> Self {
        let mut counts = Vec::with_capacity(bounds.len() + 1);
        for _ in 0..=bounds.len() {
            counts.push(AtomicU64::new(0));
        }
        Self {
            bounds,
            counts,
            sum: AtomicU64::new(0f64.to_bits()),
            count: AtomicU64::new(0),
        }
    }

    pub fn observe(&self, value: f64) {
        let mut idx = self.bounds.len(); // +Inf bucket
        for (i, b) in self.bounds.iter().enumerate() {
            if value <= *b {
                idx = i;
                break;
            }
        }
        self.counts[idx].fetch_add(1, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);
        loop {
            let cur = self.sum.load(Ordering::Relaxed);
            let next = (f64::from_bits(cur) + value).to_bits();
            if self
                .sum
                .compare_exchange(cur, next, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }
    }
}

#[derive(Debug)]
pub enum SampleValue {
    Counter(AtomicU64),
    Gauge(AtomicI64), // store f64 bits as i64 via to_bits cast
    Histogram(HistogramState),
}

impl SampleValue {
    pub fn counter() -> Self {
        Self::Counter(AtomicU64::new(0))
    }

    pub fn gauge() -> Self {
        Self::Gauge(AtomicI64::new(0f64.to_bits() as i64))
    }

    pub fn histogram(bounds: &'static [f64]) -> Self {
        Self::Histogram(HistogramState::new(bounds))
    }

    pub fn inc(&self, delta: u64) {
        if let Self::Counter(c) = self {
            c.fetch_add(delta, Ordering::Relaxed);
        }
    }

    pub fn set_gauge(&self, v: f64) {
        if let Self::Gauge(g) = self {
            g.store(v.to_bits() as i64, Ordering::Relaxed);
        }
    }

    pub fn observe(&self, v: f64) {
        if let Self::Histogram(h) = self {
            h.observe(v);
        }
    }

    pub fn counter_value(&self) -> Option<u64> {
        match self {
            Self::Counter(c) => Some(c.load(Ordering::Relaxed)),
            _ => None,
        }
    }

    pub fn gauge_value(&self) -> Option<f64> {
        match self {
            Self::Gauge(g) => Some(f64::from_bits(g.load(Ordering::Relaxed) as u64)),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct Sample {
    pub name: MetricName,
    pub labels: LabelSet,
    pub kind: SampleKind,
    pub value: SampleValue,
    pub help: &'static str,
}

/// Snapshot of a series for encoding (no atomics).
#[derive(Debug, Clone)]
pub struct SeriesSnapshot {
    pub name: String,
    pub labels: LabelSet,
    pub kind: SampleKind,
    pub help: &'static str,
    pub counter: Option<u64>,
    pub gauge: Option<f64>,
    pub hist_bounds: Option<&'static [f64]>,
    pub hist_counts: Option<Vec<u64>>,
    pub hist_sum: Option<f64>,
    pub hist_count: Option<u64>,
}

#[derive(Debug)]
struct RegistryInner {
    series: BTreeMap<(String, LabelSet), Arc<Sample>>,
}

/// Process-wide in-memory metric registry.
#[derive(Debug)]
pub struct Registry {
    inner: Mutex<RegistryInner>,
    pub interval: Duration,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(RegistryInner {
                series: BTreeMap::new(),
            }),
            interval: Duration::from_secs(1),
        }
    }

    fn ensure(
        &self,
        name: &str,
        labels: LabelSet,
        kind: SampleKind,
        help: &'static str,
        hist_bounds: Option<&'static [f64]>,
    ) -> Arc<Sample> {
        let key = (name.to_string(), labels.clone());
        let mut g = self.inner.lock();
        if let Some(s) = g.series.get(&key) {
            return Arc::clone(s);
        }
        let value = match kind {
            SampleKind::Counter => SampleValue::counter(),
            SampleKind::Gauge => SampleValue::gauge(),
            SampleKind::Histogram => {
                SampleValue::histogram(hist_bounds.unwrap_or(crate::catalog::DURATION_BUCKETS))
            }
        };
        let sample = Arc::new(Sample {
            name: MetricName::new(name).expect("catalog names are valid"),
            labels,
            kind,
            value,
            help,
        });
        g.series.insert(key, Arc::clone(&sample));
        sample
    }

    pub fn inc(&self, name: &str, labels: LabelSet, help: &'static str, delta: u64) {
        self.ensure(name, labels, SampleKind::Counter, help, None)
            .value
            .inc(delta);
    }

    pub fn set_gauge(&self, name: &str, labels: LabelSet, help: &'static str, value: f64) {
        self.ensure(name, labels, SampleKind::Gauge, help, None)
            .value
            .set_gauge(value);
    }

    pub fn observe(
        &self,
        name: &str,
        labels: LabelSet,
        help: &'static str,
        bounds: &'static [f64],
        value: f64,
    ) {
        self.ensure(name, labels, SampleKind::Histogram, help, Some(bounds))
            .value
            .observe(value);
    }

    /// Snapshot all series for encoding.
    pub fn snapshot(&self) -> Vec<SeriesSnapshot> {
        let g = self.inner.lock();
        g.series
            .values()
            .map(|s| {
                let (counter, gauge, hist_bounds, hist_counts, hist_sum, hist_count) = match &s.value
                {
                    SampleValue::Counter(c) => (
                        Some(c.load(Ordering::Relaxed)),
                        None,
                        None,
                        None,
                        None,
                        None,
                    ),
                    SampleValue::Gauge(g) => (
                        None,
                        Some(f64::from_bits(g.load(Ordering::Relaxed) as u64)),
                        None,
                        None,
                        None,
                        None,
                    ),
                    SampleValue::Histogram(h) => {
                        let counts: Vec<u64> = h
                            .counts
                            .iter()
                            .map(|c| c.load(Ordering::Relaxed))
                            .collect();
                        (
                            None,
                            None,
                            Some(h.bounds),
                            Some(counts),
                            Some(f64::from_bits(h.sum.load(Ordering::Relaxed))),
                            Some(h.count.load(Ordering::Relaxed)),
                        )
                    }
                };
                SeriesSnapshot {
                    name: s.name.as_str().to_string(),
                    labels: s.labels.clone(),
                    kind: s.kind,
                    help: s.help,
                    counter,
                    gauge,
                    hist_bounds,
                    hist_counts,
                    hist_sum,
                    hist_count,
                }
            })
            .collect()
    }

    pub fn clear(&self) {
        self.inner.lock().series.clear();
    }
}

/// Recorder facade used by owner crates.
#[derive(Debug, Clone)]
pub struct Recorder {
    registry: Arc<Registry>,
}

impl Recorder {
    pub fn new(registry: Arc<Registry>) -> Self {
        Self { registry }
    }

    pub fn registry(&self) -> &Arc<Registry> {
        &self.registry
    }

    pub fn inc_counter(
        &self,
        name: &str,
        mut labels: LabelSet,
        help: &'static str,
    ) -> Result<(), ObservabilityError> {
        // Drop empty keys already rejected by LabelSet::insert.
        let _ = &mut labels;
        self.registry.inc(name, labels, help, 1);
        Ok(())
    }

    pub fn add_counter(
        &self,
        name: &str,
        labels: LabelSet,
        help: &'static str,
        delta: u64,
    ) -> Result<(), ObservabilityError> {
        self.registry.inc(name, labels, help, delta);
        Ok(())
    }

    pub fn set_gauge(
        &self,
        name: &str,
        labels: LabelSet,
        help: &'static str,
        value: f64,
    ) -> Result<(), ObservabilityError> {
        self.registry.set_gauge(name, labels, help, value);
        Ok(())
    }

    pub fn observe_duration(
        &self,
        name: &str,
        labels: LabelSet,
        help: &'static str,
        seconds: f64,
    ) -> Result<(), ObservabilityError> {
        self.registry.observe(
            name,
            labels,
            help,
            crate::catalog::DURATION_BUCKETS,
            seconds,
        );
        Ok(())
    }

    pub fn observe_size(
        &self,
        name: &str,
        labels: LabelSet,
        help: &'static str,
        bytes: f64,
    ) -> Result<(), ObservabilityError> {
        self.registry
            .observe(name, labels, help, crate::catalog::SIZE_BUCKETS, bytes);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_and_sentinel_labels() {
        let mut ls = LabelSet::new();
        assert!(ls.insert("user", "").is_err());
        assert!(ls.insert("user", "unknown").is_err());
        assert!(ls.insert("user", "-").is_err());
        assert!(ls.insert("user", "alice").is_ok());
    }

    #[test]
    fn metric_name_validation() {
        assert!(MetricName::new("spacestorage_query_total").is_ok());
        assert!(MetricName::new("db_wal_bytes_total").is_ok());
        assert!(MetricName::new("9bad").is_err());
    }
}
