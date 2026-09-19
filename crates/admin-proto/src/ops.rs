use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum AdminOp {
    Status,
    Config,
    Threads,
    Buffers,
    Reload,
    Stop { #[serde(default)] wait: bool },
}
