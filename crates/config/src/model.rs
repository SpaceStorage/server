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
}

/// Query admission knobs (`query { }`) — policy defaults from compat; owned by 005.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryDecl {
    pub max_concurrent_per_node: Option<u32>,
    pub max_concurrent_per_namespace: Option<u32>,
    pub max_memory: Option<u64>,
    /// `Some(true)` = spill on; `Some(false)` = off; `None` = profile default.
    pub spill: Option<bool>,
}

impl Default for QueryDecl {
    fn default() -> Self {
        Self {
            max_concurrent_per_node: None,
            max_concurrent_per_namespace: None,
            max_memory: None,
            spill: None,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntrypointDecl {
    pub name: String,
    pub address: String,
    pub port: u16,
    pub handler: String,
    pub transport: Transport,
    pub tls: Option<TlsDecl>,
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
];

pub fn buffer_spec(name: &str) -> Option<(u64, u64, u64)> {
    BUILTIN_BUFFERS
        .iter()
        .find(|(n, ..)| *n == name)
        .map(|(_, d, min, max)| (*d, *min, *max))
}
