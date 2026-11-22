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
    NamespaceList,
    NamespaceCreate { name: String },
    NamespaceRename { name: String, new_name: String },
    NamespaceDelete {
        name: String,
        #[serde(default)]
        cascade: bool,
    },
    NamespaceDescribe { name: String },
    QuotaReplace {
        name: String,
        quotas: Vec<QuotaInput>,
    },
    RoleList,
    RoleBindingList {
        #[serde(default)]
        namespace: Option<String>,
    },
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
    /// 011: one-time join token bound to `node_name` (unattended admit).
    JoinTokenMint {
        node_name: String,
        #[serde(default)]
        node_id: Option<Uuid>,
        /// TTL in seconds; default 12h. Must be in [3600, 72h].
        #[serde(default)]
        ttl_secs: Option<u64>,
    },
    /// 011: membership roster (members + pending).
    Membership,
    /// 018: paginated audit read (014 AuditEntry).
    AuditList {
        #[serde(default)]
        cursor: Option<String>,
        #[serde(default)]
        limit: Option<u32>,
        #[serde(default)]
        namespace: Option<String>,
    },
    /// 018: mark member draining (011).
    Drain { node_id: Uuid },
    /// 018: return member to ready (011).
    Undrain { node_id: Uuid },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaInput {
    pub unit: String,
    pub limit: u64,
    #[serde(default)]
    pub datatype: Option<String>,
}
