use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;

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
    pub query_defaults: QueryDefaults,
    pub labels: BTreeMap<String, String>,
    pub storage_data_dir: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClusterDecl {
    pub name: Option<String>,
    pub bootstrap: bool,
    pub topology_ladder: Vec<String>,
    pub quorum_domain: Option<String>,
    pub master_key_file: Option<String>,
    pub token_file: Option<String>,
    pub join: Option<String>,
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
