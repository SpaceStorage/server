use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum AdminOp {
    Status,
    Config,
    Threads,
    Buffers,
    Reload,
    Stop { #[serde(default)] wait: bool },
    /// 006: describe cluster + namespace controllers.
    Controllers,
    /// 006: raft status for `cluster` or namespace uuid string.
    RaftStatus {
        #[serde(default)]
        group: Option<String>,
    },
    /// 006: leadership leases in a namespace.
    Leases { namespace: Uuid },
    /// Slice-7 voter migrate (else Slice7Required).
    VoterReplace {
        group: String,
        from: Uuid,
        to: Uuid,
    },
    VoterGrow {
        group: String,
        add: Vec<Uuid>,
    },
    VoterShrink {
        group: String,
        remove: Vec<Uuid>,
    },
}
