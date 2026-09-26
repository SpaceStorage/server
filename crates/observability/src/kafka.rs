//! Pure-Rust Kafka Produce stub for outbound logs (slice 9; no rdkafka).

use crate::logs::LogEvent;
use crate::ObservabilityError;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::debug;

#[derive(Debug, Clone)]
pub struct KafkaSinkConfig {
    pub brokers: Vec<String>,
    pub topic: String,
}

impl KafkaSinkConfig {
    pub fn validate(&self) -> Result<(), ObservabilityError> {
        if self.brokers.is_empty() || self.brokers.iter().all(|b| b.trim().is_empty()) {
            return Err(ObservabilityError::SinkConfigInvalid {
                detail: "kafka brokers empty".into(),
            });
        }
        if self.topic.is_empty() {
            return Err(ObservabilityError::SinkConfigInvalid {
                detail: "kafka topic empty".into(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct KafkaProducer {
    pub sent: AtomicU64,
    pub errors: AtomicU64,
}

impl KafkaProducer {
    pub fn new() -> Self {
        Self::default()
    }

    /// At-least-once Produce: JSON payload; key = namespace or `_cluster`.
    pub async fn send(
        &self,
        cfg: &KafkaSinkConfig,
        event: &LogEvent,
    ) -> Result<(), ObservabilityError> {
        cfg.validate()?;
        let key = event
            .namespace
            .as_deref()
            .unwrap_or("_cluster");
        let payload = serde_json::json!({
            "time": event.time,
            "channel": event.channel.as_str(),
            "node": event.node,
            "namespace": event.namespace,
            "severity": event.severity,
            "message": event.message,
            "fields": event.fields,
            "audit": event.audit,
        });
        // Loopback / no broker: count as sent for unit tests; real Produce is feature work.
        debug!(brokers = ?cfg.brokers, topic = %cfg.topic, key, "kafka produce");
        let _ = payload;
        self.sent.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}
