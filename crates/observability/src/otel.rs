//! OTLP/HTTP protobuf push (slice 9; feature-gated surface).

use crate::encode::encode_prometheus;
use crate::filter::filter_namespace;
use crate::sample::SeriesSnapshot;
use crate::ObservabilityError;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub struct OtelConfig {
    pub endpoint: String,
    pub interval: Duration,
}

impl OtelConfig {
    pub fn validate(&self) -> Result<(), ObservabilityError> {
        let e = self.endpoint.trim();
        if !(e.starts_with("http://") || e.starts_with("https://")) {
            return Err(ObservabilityError::SinkConfigInvalid {
                detail: "OTLP endpoint must be http(s)".into(),
            });
        }
        Ok(())
    }

    pub fn metrics_url(&self) -> String {
        let base = self.endpoint.trim_end_matches('/');
        format!("{base}/v1/metrics")
    }
}

#[derive(Debug, Default)]
pub struct OtelExporter {
    pub errors: AtomicU64,
    pub pushes: AtomicU64,
}

impl OtelExporter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push same series set as scrape (omit-label included). Never blocks recorders.
    pub async fn push(
        &self,
        cfg: &OtelConfig,
        series: &[SeriesSnapshot],
        namespace_filter: Option<&str>,
        node_id: &str,
    ) -> Result<(), ObservabilityError> {
        cfg.validate()?;
        let filtered: Vec<SeriesSnapshot> = match namespace_filter {
            Some(ns) => filter_namespace(series, ns),
            None => series.to_vec(),
        };
        // Encode as Prometheus text payload for the in-tree stub; full OTLP protobuf
        // lands behind optional `opentelemetry-otlp` when the `otel` feature pulls it.
        let body = encode_prometheus(&filtered);
        let url = cfg.metrics_url();
        debug!(
            %url,
            service.name = "spacestorage",
            service.instance.id = %node_id,
            service.namespace = "spacestorage",
            bytes = body.len(),
            "otel push"
        );
        // Without network deps in default build, count a logical push.
        let _ = url;
        self.pushes.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn record_failure(&self, stream: &str) {
        self.errors.fetch_add(1, Ordering::Relaxed);
        warn!(stream, "otel export failed");
    }
}
