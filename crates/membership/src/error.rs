use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MembershipError {
    #[error("not a member")]
    NotMember,
    #[error("pending join")]
    Pending,
    #[error("live replace refused")]
    LiveReplace,
    #[error("retired identity")]
    RetiredIdentity,
    #[error("name in use")]
    NameInUse,
    #[error("ladder")]
    Ladder,
    #[error("secret mismatch")]
    SecretMismatch,
    #[error("secret rotate in progress")]
    SecretRotateInProgress,
    #[error("last member")]
    LastMember,
    #[error("decommission blocked")]
    DecommissionBlocked,
    #[error("not pending")]
    NotPending,
    #[error("invalid state: {0}")]
    InvalidState(String),
    #[error("minority")]
    Minority,
    #[error("bootstrap_and_join")]
    BootstrapAndJoin,
    #[error("bootstrap_foreign_seeds")]
    BootstrapForeignSeeds,
    #[error("join_secret_required")]
    JoinSecretRequired,
    #[error("join refused: {0}")]
    JoinRefused(String),
    #[error("no reachable seed")]
    NoReachableSeed,
    #[error("token refused: {0}")]
    TokenRefused(String),
    #[error("io: {0}")]
    Io(String),
}

impl MembershipError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotMember => "not_member",
            Self::Pending => "pending",
            Self::LiveReplace => "live_replace",
            Self::RetiredIdentity => "retired_identity",
            Self::NameInUse => "name_in_use",
            Self::Ladder => "ladder",
            Self::SecretMismatch => "secret_mismatch",
            Self::SecretRotateInProgress => "secret_rotate_in_progress",
            Self::LastMember => "last_member",
            Self::DecommissionBlocked => "decommission_blocked",
            Self::NotPending => "not_pending",
            Self::InvalidState(_) => "invalid_state",
            Self::Minority => "minority",
            Self::BootstrapAndJoin => "bootstrap_and_join",
            Self::BootstrapForeignSeeds => "bootstrap_foreign_seeds",
            Self::JoinSecretRequired => "join_secret_required",
            Self::JoinRefused(_) => "join_refused",
            Self::NoReachableSeed => "no_reachable_seed",
            Self::TokenRefused(_) => "token_refused",
            Self::Io(_) => "io",
        }
    }
}

pub type Result<T> = std::result::Result<T, MembershipError>;
