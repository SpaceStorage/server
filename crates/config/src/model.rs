use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;

pub use spacestorage_compat::{EffectiveLimits, LimitProvenance, Limits};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub node_name: String,
    pub threads: Option<u32>, // None = auto
    pub drain_timeout: Duration,
    pub log_level: String,
    pub log_format: String,
    pub admin_token_file: Option<String>,
    pub disable_admin: bool,
    pub disable_admin_http: bool,
    pub entrypoints: Vec<EntrypointDecl>,
    pub buffers: BTreeMap<String, u64>,
    pub cluster: ClusterDecl,
    pub keys: KeysDecl,
    pub query_defaults: QueryDefaults,
    pub labels: BTreeMap<String, String>,
    pub storage_data_dir: Option<String>,
    pub storage: StorageDecl,
    /// Size/connection limits (`limits { }`); missing → built-in defaults (015).
    pub limits: EffectiveLimits,
    /// Optional `query { spill; max_concurrent_*; max_memory; }` (005 owned; parsed here).
    pub query: QueryDecl,
    /// Observability (`metrics { }` / additive `log { kafka|syslog }`) — 008.
    pub metrics: MetricsDecl,
    pub log_kafka: Option<LogKafkaDecl>,
    pub log_syslog: Option<LogSyslogDecl>,
    /// Data job supervisor (`jobs { }`) — 010.
    pub jobs: JobsDecl,
    /// Bootstrap Kafka ingest declarations (`ingest kafka NAME { }`) — 009.
    pub kafka_ingests: Vec<KafkaIngestDecl>,
}

impl NodeConfig {
    /// Normative 014 path is `keys.master_key_file`; `cluster.master_key_file`
    /// remains an accepted first-binary (016) alias.
    pub fn effective_master_key_file(&self) -> Option<&str> {
        self.keys
            .master_key_file
            .as_deref()
            .or(self.cluster.master_key_file.as_deref())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeysDecl {
    pub master_key_file: Option<String>,
    pub create_master_if_absent: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StorageDecl {
    pub sync: Option<String>,
    pub gc_grace_ms: Option<u64>,
    pub wal_segment_bytes: Option<u64>,
    pub group_commit_max_wait_ms: Option<u64>,
    pub group_commit_max_bytes: Option<u64>,
    pub drives: Vec<DriveDecl>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriveDecl {
    pub id: String,
    pub path: String,
}

/// Seed discovery endpoint (`cluster { seeds { name; address; port; } }`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SeedDecl {
    pub name: String,
    pub address: String,
    pub port: u16,
}

/// `cluster.raft { heartbeat; election_timeout; }` (006).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RaftDecl {
    pub heartbeat: Duration,
    pub election_timeout: Duration,
}

impl Default for RaftDecl {
    fn default() -> Self {
        Self {
            heartbeat: Duration::from_millis(500),
            election_timeout: Duration::from_secs(2),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClusterDecl {
    pub name: Option<String>,
    pub bootstrap: bool,
    pub topology_ladder: Vec<String>,
    pub quorum_domain: Option<String>,
    /// Alias for `keys.master_key_file` (first-binary starters). Prefer `keys {}`.
    pub master_key_file: Option<String>,
    pub token_file: Option<String>,
    /// `Some(_)` when `cluster { join; }` is declared (arg optional / unused).
    pub join: Option<String>,
    /// Optional one-time join token file path (`join_token_file`).
    pub join_token_file: Option<String>,
    /// First-join seed list (`seeds` / `peers`).
    pub seeds: Vec<SeedDecl>,
    pub admin_login: Option<String>,
    pub admin_password_file: Option<String>,
    /// Optional override; default compiled-in product version = 1 (015).
    pub product_version: Option<u16>,
    /// Raft timings (006); omitted → production defaults.
    pub raft: RaftDecl,
    /// Slice-7: exclude controller voters as tenant replica targets (default off).
    pub controller_exclusive_data: bool,
}

/// Query admission knobs (`query { }`) — policy defaults from compat; owned by 005.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryDecl {
    pub max_concurrent_per_node: Option<u32>,
    pub max_concurrent_per_namespace: Option<u32>,
    pub max_memory: Option<u64>,
    /// `Some(true)` = spill on; `Some(false)` = off; `None` = profile default.
    pub spill: Option<bool>,
    /// Default planner concurrency degree (1 = sequential).
    pub default_concurrency: Option<u16>,
}

impl Default for QueryDecl {
    fn default() -> Self {
        Self {
            max_concurrent_per_node: None,
            max_concurrent_per_namespace: None,
            max_memory: None,
            spill: None,
            default_concurrency: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryDefaults {
    pub write_quorum: String,
    pub read_quorum: String,
}

impl Default for QueryDefaults {
    fn default() -> Self {
        Self {
            write_quorum: "TWO".into(),
            read_quorum: "ONE".into(),
        }
    }
}

/// `metrics { otel; slow_query; audit_log; }` (008).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricsDecl {
    pub otel_endpoint: Option<String>,
    pub otel_interval: Option<Duration>,
    pub slow_query_enabled: bool,
    pub slow_query_threshold: Duration,
    pub audit_log: bool,
}

impl Default for MetricsDecl {
    fn default() -> Self {
        Self::defaults()
    }
}

impl MetricsDecl {
    pub fn defaults() -> Self {
        Self {
            otel_endpoint: None,
            otel_interval: None,
            slow_query_enabled: false,
            slow_query_threshold: Duration::from_secs(1),
            audit_log: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogKafkaDecl {
    pub brokers: Vec<String>,
    pub topic: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogSyslogDecl {
    pub address: String,
    pub transport: String, // udp|tcp
}

/// `jobs { enabled; catchup_concurrency; }` (010).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobsDecl {
    /// Default off on first-binary; on when slice 10 compiled.
    pub enabled: bool,
    pub catchup_concurrency: u32,
}

impl Default for JobsDecl {
    fn default() -> Self {
        Self {
            enabled: false,
            catchup_concurrency: 4,
        }
    }
}

/// Bootstrap `ingest kafka NAME { … }` (009); cluster store wins after apply.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KafkaIngestDecl {
    pub name: String,
    pub namespace: String,
    pub container: String,
    pub type_name: String,
    pub format: String,
    pub brokers: Vec<String>,
    pub topic: String,
    pub group: String,
    pub plaintext: bool,
}

impl Default for KafkaIngestDecl {
    fn default() -> Self {
        Self {
            name: String::new(),
            namespace: String::new(),
            container: String::new(),
            type_name: "log_stream".into(),
            format: "raw".into(),
            brokers: Vec::new(),
            topic: String::new(),
            group: String::new(),
            plaintext: true,
        }
    }
}

/// Syslog entrypoint ingest child (009).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyslogIngestDecl {
    pub namespace: String,
    pub container: String,
    pub type_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntrypointDecl {
    pub name: String,
    pub address: String,
    pub port: u16,
    pub handler: String,
    pub transport: Transport,
    pub tls: Option<TlsDecl>,
    /// Required when `handler syslog` (009).
    pub ingest: Option<SyslogIngestDecl>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Transport {
    Plaintext,
    Tls,
    Undeclared,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsDecl {
    pub certificate: String,
    pub key: String,
}

pub const BUILTIN_BUFFERS: &[(&str, u64, u64, u64)] = &[
    // name, default, min, max
    ("net.recv", 64 * 1024 * 1024, 1024 * 1024, 64 * 1024 * 1024 * 1024),
    ("net.send", 64 * 1024 * 1024, 1024 * 1024, 64 * 1024 * 1024 * 1024),
    (
        "request.queue",
        16 * 1024 * 1024,
        1024 * 1024,
        16 * 1024 * 1024 * 1024,
    ),
    (
        "ingest.syslog.recv",
        16 * 1024 * 1024,
        1024 * 1024,
        1024 * 1024 * 1024,
    ),
    (
        "ingest.kafka.decode",
        32 * 1024 * 1024,
        1024 * 1024,
        1024 * 1024 * 1024,
    ),
];

pub fn buffer_spec(name: &str) -> Option<(u64, u64, u64)> {
    BUILTIN_BUFFERS
        .iter()
        .find(|(n, ..)| *n == name)
        .map(|(_, d, min, max)| (*d, *min, *max))
}
