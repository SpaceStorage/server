use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigError {
    pub file: String,
    pub line: u32,
    pub col: u32,
    pub setting: String,
    pub code: String,
    pub message: String,
}

impl ConfigError {
    pub fn new(
        file: impl Into<String>,
        line: u32,
        col: u32,
        setting: impl Into<String>,
        code: ErrorCode,
        message: impl Into<String>,
    ) -> Self {
        Self {
            file: file.into(),
            line,
            col,
            setting: setting.into(),
            code: code.as_str().to_string(),
            message: message.into(),
        }
    }

    pub fn simple(
        file: impl Into<String>,
        line: u32,
        col: u32,
        setting: impl Into<String>,
        code: ErrorCode,
        message: impl Into<String>,
    ) -> Self {
        Self::new(file, line, col, setting, code, message)
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{} [{}] {}: {}",
            self.file, self.line, self.col, self.code, self.setting, self.message
        )
    }
}

impl std::error::Error for ConfigError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    Syntax,
    UnknownDirective,
    WrongArity,
    BadLiteral,
    AdminHandlerUndeclared,
    AdminHandlerConflict,
    AdminTokenRequired,
    AdminTokenUnreadable,
    EntrypointMissingPort,
    EntrypointMissingHandler,
    EntrypointMultipleHandlers,
    EntrypointUnknownHandler,
    EntrypointDuplicateAddress,
    EntrypointDuplicateName,
    ThreadsOutOfRange,
    DrainTimeoutOutOfRange,
    BufferUnknown,
    BufferOutOfRange,
    CertInlineForbidden,
    CertRefSchemeUnsupported,
    CertUnreadable,
    KeyUnreadable,
    CertInvalid,
    CertExpired,
    TransportUndeclared,
    InternodeRequired,
    ReplicationRequired,
    MasterKeyRequired,
    MasterKeyUnreadable,
    MasterKeyPermissions,
    KeyringRemoved,
    KeyMaterialForbidden,
    TopologyLadderRequired,
    SyncNoneNotDurable,
    GcGraceTooSmall,
    LimitsZero,
    LimitsUnknownUnit,
    ProductVersionZero,
    /// Raft `heartbeat` / `election_timeout` invalid (006).
    RaftTimeoutInvalid,
    /// `controller_exclusive_data on` without controlplane-ops / slice 7.
    Slice7Required,
    QueryMaxConcurrentZero,
    QueryMaxMemoryZero,
    QueryConcurrencyZero,
    QuerySpillUnknown,
    /// Slice-9 observability features used on first-binary profile.
    ObservabilitySlice9Required,
    /// Kafka/syslog/OTLP sink misconfigured.
    SinkConfigInvalid,
    /// `jobs.enabled on` on first-binary profile (010).
    MigrateSlice10Required,
    /// UI / Kafka / syslog ingest on first-binary profile (009).
    UiIngestSlice11Required,
    /// Kafka ingest missing brokers.
    IngestKafkaNoBrokers,
    /// Syslog ingest missing namespace/container.
    IngestMissingTarget,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::UnknownDirective => "unknown_directive",
            Self::WrongArity => "wrong_arity",
            Self::BadLiteral => "bad_literal",
            Self::AdminHandlerUndeclared => "admin_handler_undeclared",
            Self::AdminHandlerConflict => "admin_handler_conflict",
            Self::AdminTokenRequired => "admin_token_required",
            Self::AdminTokenUnreadable => "admin_token_unreadable",
            Self::EntrypointMissingPort => "entrypoint_missing_port",
            Self::EntrypointMissingHandler => "entrypoint_missing_handler",
            Self::EntrypointMultipleHandlers => "entrypoint_multiple_handlers",
            Self::EntrypointUnknownHandler => "entrypoint_unknown_handler",
            Self::EntrypointDuplicateAddress => "entrypoint_duplicate_address",
            Self::EntrypointDuplicateName => "entrypoint_duplicate_name",
            Self::ThreadsOutOfRange => "threads_out_of_range",
            Self::DrainTimeoutOutOfRange => "drain_timeout_out_of_range",
            Self::BufferUnknown => "buffer_unknown",
            Self::BufferOutOfRange => "buffer_out_of_range",
            Self::CertInlineForbidden => "cert_inline_forbidden",
            Self::CertRefSchemeUnsupported => "cert_ref_scheme_unsupported",
            Self::CertUnreadable => "cert_unreadable",
            Self::KeyUnreadable => "key_unreadable",
            Self::CertInvalid => "cert_invalid",
            Self::CertExpired => "cert_expired",
            Self::TransportUndeclared => "transport_undeclared",
            Self::InternodeRequired => "internode_required",
            Self::ReplicationRequired => "replication_required",
            Self::MasterKeyRequired => "master_key_required",
            Self::MasterKeyUnreadable => "master_key_unreadable",
            Self::MasterKeyPermissions => "master_key_permissions",
            Self::KeyringRemoved => "KeyringRemoved",
            Self::KeyMaterialForbidden => "KeyMaterialForbidden",
            Self::TopologyLadderRequired => "topology_ladder_required",
            Self::SyncNoneNotDurable => "sync_none_not_durable",
            Self::GcGraceTooSmall => "gc_grace_too_small",
            Self::LimitsZero => "limits_zero",
            Self::LimitsUnknownUnit => "limits_unknown_unit",
            Self::ProductVersionZero => "product_version_zero",
            Self::RaftTimeoutInvalid => "raft_timeout_invalid",
            Self::Slice7Required => "Slice7Required",
            Self::QueryMaxConcurrentZero => "query_max_concurrent_zero",
            Self::QueryMaxMemoryZero => "query_max_memory_zero",
            Self::QueryConcurrencyZero => "query_concurrency_zero",
            Self::QuerySpillUnknown => "query_spill_unknown",
            Self::ObservabilitySlice9Required => "ObservabilitySlice9Required",
            Self::SinkConfigInvalid => "SinkConfigInvalid",
            Self::MigrateSlice10Required => "MigrateSlice10Required",
            Self::UiIngestSlice11Required => "UiIngestSlice11Required",
            Self::IngestKafkaNoBrokers => "ingest_kafka_no_brokers",
            Self::IngestMissingTarget => "ingest_missing_target",
        }
    }
}
