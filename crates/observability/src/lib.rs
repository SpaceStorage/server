//! Global /metrics exposition, registry, sinks (008).
//!
//! Reserved `db_wal_*` series are owned/incremented by `spacestorage-storage`
//! (013 T060 — provisional `spacestorage_wal_acks_total` retired).

pub mod aggregation;
pub mod billing;
pub mod catalog;
pub mod encode;
pub mod filter;
pub mod http;
pub mod kafka;
pub mod logs;
pub mod otel;
pub mod otlp_encode;
pub mod sample;
pub mod syslog;

pub use aggregation::{merge_push, AggregationFreshness, AggregationStore};
pub use billing::{estimate_charge, estimate_total, BillingRates, ChargeEstimate, UsageSnapshot};
pub use catalog::{
    all_families, help_for, FamilyMeta, API_FAMILIES, BILLING_FAMILIES, COMM_FAMILIES,
    DATATYPE_FAMILIES, DURATION_BUCKETS, DURABILITY_FAMILIES, JOB_FAMILIES, JOB_NAMES,
    QUERY_FAMILIES, REPLICATION_FAMILIES, SIZE_BUCKETS, SYSTEM_FAMILIES,
};
pub use encode::{encode_prometheus, prometheus_content_type};
pub use filter::filter_namespace;
pub use http::{
    encode_global, encode_tenant, metrics_disabled, prometheus_response, unknown_namespace,
};
pub use kafka::{KafkaProducer, KafkaSinkConfig, ProducedRecord};
pub use otlp_encode::encode_otlp_metrics;
pub use logs::{ChannelGates, LogChannel, LogEvent, LogExporter};
pub use otel::{OtelConfig, OtelExporter};
pub use sample::{LabelSet, MetricName, Recorder, Registry, SampleKind, SeriesSnapshot};
pub use syslog::{SyslogExporter, SyslogSinkConfig, SyslogTransport};

use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ObservabilityError {
    #[error("ObservabilitySlice9Required: {detail}")]
    ObservabilitySlice9Required { detail: String },
    #[error("UnknownLabelForbidden: {detail}")]
    UnknownLabelForbidden { detail: String },
    #[error("MetricsReadDenied")]
    MetricsReadDenied,
    #[error("ForeignNamespace")]
    ForeignNamespace,
    #[error("SinkConfigInvalid: {detail}")]
    SinkConfigInvalid { detail: String },
    #[error("KeyMaterialForbidden")]
    KeyMaterialForbidden,
}

impl ObservabilityError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ObservabilitySlice9Required { .. } => "ObservabilitySlice9Required",
            Self::UnknownLabelForbidden { .. } => "UnknownLabelForbidden",
            Self::MetricsReadDenied => "MetricsReadDenied",
            Self::ForeignNamespace => "ForeignNamespace",
            Self::SinkConfigInvalid { .. } => "SinkConfigInvalid",
            Self::KeyMaterialForbidden => "KeyMaterialForbidden",
        }
    }
}

/// Buffer usage snapshot for scrape sync (001 → 008).
#[derive(Debug, Clone)]
pub struct BufferSnapshot {
    pub name: String,
    pub used_bytes: u64,
    pub capacity_bytes: u64,
    pub usage_ratio: f64,
    pub limit_hits_total: u64,
}

/// Node system figures refreshed into the registry on each scrape.
#[derive(Debug, Clone)]
pub struct NodeSystemSnapshot {
    pub uptime_seconds: f64,
    /// 1 when ready.
    pub node_ready: f64,
    /// starting=0 ready=1 draining=2 degraded=3 recovering=4 failed=5
    pub node_state: f64,
    pub worker_threads: f64,
    pub worker_threads_busy: f64,
    pub buffers: Vec<BufferSnapshot>,
}

/// Process metrics facade: registry + legacy counters + scrape helpers.
pub struct Metrics {
    pub started: Instant,
    /// Legacy counter; also increments `spacestorage_query_total` when used.
    pub queries_total: AtomicU64,
    registry: Arc<Registry>,
    aggregation: Arc<AggregationStore>,
    logs: Arc<LogExporter>,
    /// Optional NamespaceTelemetry scrape enables (name → enabled).
    tenant_scrape: Mutex<std::collections::BTreeMap<String, bool>>,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            queries_total: AtomicU64::new(0),
            registry: Arc::new(Registry::new()),
            aggregation: Arc::new(AggregationStore::new()),
            logs: Arc::new(LogExporter::new()),
            tenant_scrape: Mutex::new(std::collections::BTreeMap::new()),
        }
    }

    pub fn registry(&self) -> &Arc<Registry> {
        &self.registry
    }

    pub fn recorder(&self) -> Recorder {
        Recorder::new(Arc::clone(&self.registry))
    }

    pub fn aggregation(&self) -> &Arc<AggregationStore> {
        &self.aggregation
    }

    pub fn logs(&self) -> &Arc<LogExporter> {
        &self.logs
    }

    pub fn set_tenant_scrape(&self, namespace: &str, enabled: bool) {
        self.tenant_scrape
            .lock()
            .insert(namespace.to_string(), enabled);
    }

    pub fn tenant_scrape_enabled(&self, namespace: &str) -> bool {
        self.tenant_scrape
            .lock()
            .get(namespace)
            .copied()
            .unwrap_or(false)
    }

    pub fn record_query(&self) {
        self.queries_total.fetch_add(1, Ordering::Relaxed);
        let mut labels = LabelSet::new();
        let _ = labels.insert("kind", "query");
        self.registry.inc(
            "spacestorage_query_total",
            labels,
            help_for("spacestorage_query_total"),
            1,
        );
    }

    /// Publish node/system series then encode Prometheus text.
    pub fn render_prometheus(&self) -> String {
        self.publish_uptime_only();
        encode_prometheus(&self.registry.snapshot())
    }

    /// Full scrape with live node system figures (admin-http).
    pub fn render_with_system(&self, sys: &NodeSystemSnapshot) -> String {
        self.publish_system(sys);
        self.aggregation.publish_to_registry(&self.registry);
        encode_prometheus(&self.registry.snapshot())
    }

    fn publish_uptime_only(&self) {
        let up = self.started.elapsed().as_secs_f64();
        self.registry.set_gauge(
            "spacestorage_uptime_seconds",
            LabelSet::new(),
            help_for("spacestorage_uptime_seconds"),
            up,
        );
        // Keep legacy process name for any existing scrapers during transition.
        self.registry.set_gauge(
            "spacestorage_process_uptime_seconds",
            LabelSet::new(),
            "Process uptime (legacy alias).",
            up,
        );
        let q = self.queries_total.load(Ordering::Relaxed);
        if q > 0 {
            // Reflect legacy counter if owners only bumped AtomicU64.
            let mut labels = LabelSet::new();
            let _ = labels.insert("kind", "query");
            // Set via inc from 0 would double-count; only seed if series missing.
            let snap = self.registry.snapshot();
            let has = snap.iter().any(|s| s.name == "spacestorage_query_total");
            if !has {
                self.registry.inc(
                    "spacestorage_query_total",
                    labels,
                    help_for("spacestorage_query_total"),
                    q,
                );
            }
        }
    }

    pub fn publish_system(&self, sys: &NodeSystemSnapshot) {
        self.registry.set_gauge(
            "spacestorage_uptime_seconds",
            LabelSet::new(),
            help_for("spacestorage_uptime_seconds"),
            sys.uptime_seconds,
        );
        // Legacy alias kept for first-binary scrapers / admin_parity.
        self.registry.set_gauge(
            "spacestorage_process_uptime_seconds",
            LabelSet::new(),
            "Process uptime (legacy alias).",
            sys.uptime_seconds,
        );
        for (name, value) in [
            ("node_ready", sys.node_ready),
            ("spacestorage_node_ready", sys.node_ready),
            ("node_state", sys.node_state),
            ("spacestorage_node_state", sys.node_state),
            ("spacestorage_worker_threads", sys.worker_threads),
            ("spacestorage_worker_threads_busy", sys.worker_threads_busy),
        ] {
            self.registry
                .set_gauge(name, LabelSet::new(), help_for(name), value);
        }
        let usage = if sys.worker_threads > 0.0 {
            sys.worker_threads_busy / sys.worker_threads
        } else {
            0.0
        };
        self.registry.set_gauge(
            "spacestorage_worker_threads_usage_ratio",
            LabelSet::new(),
            help_for("spacestorage_worker_threads_usage_ratio"),
            usage,
        );
        for b in &sys.buffers {
            let mut labels = LabelSet::new();
            let _ = labels.insert("buffer", b.name.as_str());
            self.registry.set_gauge(
                "spacestorage_buffer_usage_bytes",
                labels.clone(),
                help_for("spacestorage_buffer_usage_bytes"),
                b.used_bytes as f64,
            );
            self.registry.set_gauge(
                "spacestorage_buffer_usage_ratio",
                labels.clone(),
                help_for("spacestorage_buffer_usage_ratio"),
                b.usage_ratio,
            );
            self.registry.set_gauge(
                "spacestorage_buffer_capacity_bytes",
                labels.clone(),
                help_for("spacestorage_buffer_capacity_bytes"),
                b.capacity_bytes as f64,
            );
            // Counter: set by re-creating — use gauge-like replace via ensure+store.
            // For limit hits, increment delta is hard without prior; publish absolute via
            // clearing isn't available — encode as gauge-backed counter sample.
            self.registry.inc(
                "spacestorage_buffer_limit_hits_total",
                labels,
                help_for("spacestorage_buffer_limit_hits_total"),
                0,
            );
            // Overwrite counter atom by reading sample — use set via gauge path for absolute.
            // Absolute counter exposition: store as counter value by ensure then direct.
            publish_counter_absolute(
                &self.registry,
                "spacestorage_buffer_limit_hits_total",
                {
                    let mut l = LabelSet::new();
                    let _ = l.insert("buffer", b.name.as_str());
                    l
                },
                help_for("spacestorage_buffer_limit_hits_total"),
                b.limit_hits_total,
            );
        }
    }
}

fn publish_counter_absolute(
    registry: &Registry,
    name: &str,
    labels: LabelSet,
    help: &'static str,
    value: u64,
) {
    // Ensure series exists then load current and add delta to reach absolute.
    registry.inc(name, labels.clone(), help, 0);
    let snap = registry.snapshot();
    let current = snap
        .iter()
        .find(|s| s.name == name && s.labels == labels)
        .and_then(|s| s.counter)
        .unwrap_or(0);
    if value > current {
        registry.inc(name, labels, help, value - current);
    }
}

/// Map lifecycle string to node_state gauge.
pub fn node_state_value(state: &str) -> f64 {
    match state {
        "starting" => 0.0,
        "ready" => 1.0,
        "draining" => 2.0,
        "degraded" => 3.0,
        "recovering" => 4.0,
        "failed" => 5.0,
        _ => 0.0,
    }
}
