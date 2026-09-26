//! Outbound log channels and event model (FR-020–022).

use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{debug, warn};

use crate::ObservabilityError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogChannel {
    Default,
    SlowQuery,
    Audit,
}

impl LogChannel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::SlowQuery => "slow_query",
            Self::Audit => "audit",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogEvent {
    pub time: String,
    pub channel: LogChannel,
    pub node: String,
    pub namespace: Option<String>,
    pub severity: String, // info | notice | err
    pub message: String,
    pub fields: serde_json::Value,
    pub audit: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct ChannelGates {
    pub default_on: bool,
    pub slow_query_on: bool,
    pub slow_query_threshold_secs: f64,
    pub audit_on: bool,
}

impl Default for ChannelGates {
    fn default() -> Self {
        Self {
            default_on: true,
            slow_query_on: false,
            slow_query_threshold_secs: 1.0,
            audit_on: false,
        }
    }
}

const QUEUE_CAP: usize = 1024;

/// Bounded outbound log queue; overflow drops + increments export errors.
#[derive(Debug, Default)]
pub struct LogExporter {
    gates: Mutex<ChannelGates>,
    queue: Mutex<VecDeque<LogEvent>>,
    pub export_errors: AtomicU64,
}

impl LogExporter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_gates(&self, gates: ChannelGates) {
        *self.gates.lock() = gates;
    }

    pub fn gates(&self) -> ChannelGates {
        self.gates.lock().clone()
    }

    /// Emit if channel gating allows. Never blocks; drops on overflow.
    pub fn emit(&self, event: LogEvent) -> Result<(), ObservabilityError> {
        if contains_key_material(&event) {
            return Err(ObservabilityError::KeyMaterialForbidden);
        }
        let gates = self.gates.lock().clone();
        let allow = match event.channel {
            LogChannel::Default => gates.default_on,
            LogChannel::SlowQuery => gates.slow_query_on,
            LogChannel::Audit => gates.audit_on,
        };
        if !allow {
            return Ok(());
        }
        let mut q = self.queue.lock();
        if q.len() >= QUEUE_CAP {
            self.export_errors.fetch_add(1, Ordering::Relaxed);
            warn!(channel = event.channel.as_str(), "log export queue overflow; dropping");
            return Ok(());
        }
        q.push_back(event);
        Ok(())
    }

    pub fn try_pop(&self) -> Option<LogEvent> {
        self.queue.lock().pop_front()
    }

    /// Successful under-threshold queries must emit 0 lines.
    pub fn maybe_slow_query(
        &self,
        node: &str,
        namespace: Option<&str>,
        duration_secs: f64,
        message: &str,
    ) {
        let gates = self.gates();
        if !gates.slow_query_on || duration_secs < gates.slow_query_threshold_secs {
            debug!(duration_secs, "slow_query gated off or under threshold");
            return;
        }
        let _ = self.emit(LogEvent {
            time: chrono_now(),
            channel: LogChannel::SlowQuery,
            node: node.into(),
            namespace: namespace.map(str::to_string),
            severity: "notice".into(),
            message: message.into(),
            fields: serde_json::json!({"duration_seconds": duration_secs}),
            audit: None,
        });
    }
}

fn chrono_now() -> String {
    // Avoid chrono dep: RFC3339-ish via unix.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

fn contains_key_material(event: &LogEvent) -> bool {
    let s = format!("{}{}", event.message, event.fields);
    s.contains("BEGIN PRIVATE KEY") || s.contains("master_key") || s.contains("\"key_bytes\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_gating() {
        let exp = LogExporter::new();
        // default on: error line accepted
        exp.emit(LogEvent {
            time: "t".into(),
            channel: LogChannel::Default,
            node: "n1".into(),
            namespace: Some("acme".into()),
            severity: "err".into(),
            message: "query failed".into(),
            fields: serde_json::json!({}),
            audit: None,
        })
        .unwrap();
        assert!(exp.try_pop().is_some());

        // slow_query off → 0 lines
        exp.maybe_slow_query("n1", Some("acme"), 5.0, "slow");
        assert!(exp.try_pop().is_none());

        exp.set_gates(ChannelGates {
            slow_query_on: true,
            ..ChannelGates::default()
        });
        // under threshold success → 0
        exp.maybe_slow_query("n1", Some("acme"), 0.1, "fast");
        assert!(exp.try_pop().is_none());
        // over threshold → line
        exp.maybe_slow_query("n1", Some("acme"), 2.0, "slow");
        assert!(exp.try_pop().is_some());

        // audit off → 0
        exp.emit(LogEvent {
            time: "t".into(),
            channel: LogChannel::Audit,
            node: "n1".into(),
            namespace: Some("acme".into()),
            severity: "notice".into(),
            message: "authz".into(),
            fields: serde_json::json!({}),
            audit: Some(serde_json::json!({"action":"read"})),
        })
        .unwrap();
        assert!(exp.try_pop().is_none());
    }
}
