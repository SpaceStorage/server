use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Status {
    pub node_name: String,
    pub state: String,
    pub uptime_seconds: u64,
    pub version: String,
    pub threads: ThreadsReport,
    pub entrypoints: Vec<EntrypointStatus>,
    pub buffers: Vec<BufferReport>,
    pub drain_timed_out: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handlers: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadsReport {
    pub total: u32,
    pub busy: u32,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_cores: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntrypointStatus {
    pub name: String,
    pub address: String,
    pub port: u16,
    pub handler: String,
    pub transport: String,
    pub cert_expired: bool,
    pub connections_active: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferReport {
    pub name: String,
    pub capacity_bytes: u64,
    pub used_bytes: u64,
    pub usage_ratio: f64,
    pub limit_hits_total: u64,
    pub policy: String,
    pub range: BufferRange,
    pub default_bytes: u64,
    pub owner: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferRange {
    pub min: u64,
    pub max: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectiveConfig {
    pub node_name: String,
    pub threads: u32,
    pub threads_source: String,
    pub drain_timeout_secs: u64,
    pub log_level: String,
    pub log_format: String,
    pub handlers: Vec<String>,
    pub entrypoints: Vec<EntrypointStatus>,
    pub buffers: Vec<BufferReport>,
    pub pending_restart: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReloadReport {
    pub ok: bool,
    pub changed: Vec<serde_json::Value>,
    pub applied_live: Vec<String>,
    pub pending_restart: Vec<String>,
    pub warnings: Vec<String>,
    pub errors: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub details: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopResult {
    pub accepted: bool,
    pub state: String,
    pub drain_timeout_secs: u64,
}
