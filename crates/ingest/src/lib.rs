//! Kafka and syslog ingest into Log Stream (009).

pub mod declaration;
pub mod error;
pub mod format;
pub mod kafka;
pub mod kafka_wire;
pub mod parse_rfc3164;
pub mod parse_rfc5424;
pub mod record;
pub mod syslog;

pub use declaration::{IngestAuthz, KafkaIngest, KafkaIngestStore, SyslogIngestBind};
pub use error::{IngestError, UI_INGEST_SLICE11_REQUIRED};
pub use format::{map_kafka_payload, PayloadFormat};
pub use kafka::{
    AppendAck, DecodeBuffer, FakeKafkaBroker, KafkaBrokerBackend, KafkaConsumer, KafkaMessage,
};
pub use kafka_wire::{range_assign_partitions, LiveKafkaClient};
pub use record::LogRecord;
pub use syslog::SyslogHandler;

/// Application name for ingest writes (`005`/`008`).
pub const APPLICATION_INGEST: &str = "spacestorage-ingest";

/// Metric series owned by 009 (do not touch spacestorage_log_export_*).
pub mod metrics {
    pub const RECORDS_TOTAL: &str = "spacestorage_ingest_records_total";
    pub const PARSE_ERRORS_TOTAL: &str = "spacestorage_ingest_parse_errors_total";
    pub const DROPPED_TOTAL: &str = "spacestorage_ingest_dropped_total";
    pub const KAFKA_OFFSET_COMMITS_TOTAL: &str = "spacestorage_ingest_kafka_offset_commits_total";
}

/// Registration hooks for node: no-op when slice11 off.
pub struct IngestRuntime {
    pub slice11_enabled: bool,
    pub kafka: KafkaIngestStore,
    pub metrics: Option<std::sync::Arc<spacestorage_observability::Metrics>>,
}

impl IngestRuntime {
    pub fn new(slice11_enabled: bool) -> Self {
        Self {
            slice11_enabled,
            kafka: KafkaIngestStore::new(slice11_enabled),
            metrics: None,
        }
    }

    pub fn with_metrics(mut self, m: std::sync::Arc<spacestorage_observability::Metrics>) -> Self {
        self.metrics = Some(m);
        self
    }

    pub fn bump_record(
        &self,
        source: &str,
        namespace: &str,
        container: &str,
        format: &str,
        result: &str,
    ) {
        let Some(m) = &self.metrics else {
            return;
        };
        let mut labels = spacestorage_observability::LabelSet::new();
        let _ = labels.insert("source", source);
        let _ = labels.insert("namespace", namespace);
        let _ = labels.insert("container", container);
        let _ = labels.insert("format", format);
        let _ = labels.insert("result", result);
        m.registry().inc(
            metrics::RECORDS_TOTAL,
            labels,
            spacestorage_observability::help_for(metrics::RECORDS_TOTAL),
            1,
        );
    }

    pub fn bump_parse_error(&self, source: &str, namespace: &str, container: &str, reason: &str) {
        let Some(m) = &self.metrics else {
            return;
        };
        let mut labels = spacestorage_observability::LabelSet::new();
        let _ = labels.insert("source", source);
        let _ = labels.insert("namespace", namespace);
        let _ = labels.insert("container", container);
        let _ = labels.insert("reason", reason);
        m.registry().inc(
            metrics::PARSE_ERRORS_TOTAL,
            labels,
            spacestorage_observability::help_for(metrics::PARSE_ERRORS_TOTAL),
            1,
        );
    }

    pub fn bump_dropped(&self, source: &str, reason: &str) {
        let Some(m) = &self.metrics else {
            return;
        };
        let mut labels = spacestorage_observability::LabelSet::new();
        let _ = labels.insert("source", source);
        let _ = labels.insert("reason", reason);
        m.registry().inc(
            metrics::DROPPED_TOTAL,
            labels,
            spacestorage_observability::help_for(metrics::DROPPED_TOTAL),
            1,
        );
    }

    pub fn bump_offset_commit(&self, namespace: &str, container: &str, result: &str) {
        let Some(m) = &self.metrics else {
            return;
        };
        let mut labels = spacestorage_observability::LabelSet::new();
        let _ = labels.insert("namespace", namespace);
        let _ = labels.insert("container", container);
        let _ = labels.insert("result", result);
        m.registry().inc(
            metrics::KAFKA_OFFSET_COMMITS_TOTAL,
            labels,
            spacestorage_observability::help_for(metrics::KAFKA_OFFSET_COMMITS_TOTAL),
            1,
        );
    }
}
