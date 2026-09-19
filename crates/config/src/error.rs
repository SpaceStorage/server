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
    TopologyLadderRequired,
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
            Self::TopologyLadderRequired => "topology_ladder_required",
        }
    }
}
