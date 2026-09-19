//! Validation / audit error codes for the release profile and ledger.

use std::fmt;

/// Named validation codes reused across config, node, and release-profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationCode {
    SliceUnknown { id: u8 },
    SliceGap { missing: Vec<u8> },
    DeferredMissing { missing: Vec<u8> },
    DeferredMarkedCancelled { id: u8 },
    ImplementedNotPrefix,
    ProfileMismatch {
        profile: String,
        expected_k: u8,
        actual_k: u8,
    },
    ProfileIncomplete {
        profile: String,
        missing: Vec<u8>,
    },
    NongoalUnspecified {
        id: String,
        spec: String,
    },
    EntrypointUnknownHandler {
        handler: String,
        known: Vec<String>,
    },
    TransportUndeclared {
        entrypoint: String,
    },
    InternodeRequired,
    ReplicationRequired,
    MasterKeyRequired,
    MasterKeyUnreadable {
        path: String,
    },
    TopologyLadderRequired,
    MultiActiveUnsupported,
    FeatureNotSupported {
        feature: String,
    },
}

impl ValidationCode {
    pub fn code(&self) -> &'static str {
        match self {
            Self::SliceUnknown { .. } => "slice_unknown",
            Self::SliceGap { .. } => "slice_gap",
            Self::DeferredMissing { .. } => "deferred_missing",
            Self::DeferredMarkedCancelled { .. } => "deferred_marked_cancelled",
            Self::ImplementedNotPrefix => "implemented_not_prefix",
            Self::ProfileMismatch { .. } => "profile_mismatch",
            Self::ProfileIncomplete { .. } => "profile_incomplete",
            Self::NongoalUnspecified { .. } => "nongoal_unspecified",
            Self::EntrypointUnknownHandler { .. } => "entrypoint_unknown_handler",
            Self::TransportUndeclared { .. } => "transport_undeclared",
            Self::InternodeRequired => "internode_required",
            Self::ReplicationRequired => "replication_required",
            Self::MasterKeyRequired => "master_key_required",
            Self::MasterKeyUnreadable { .. } => "master_key_unreadable",
            Self::TopologyLadderRequired => "topology_ladder_required",
            Self::MultiActiveUnsupported => "multi_active_unsupported",
            Self::FeatureNotSupported { .. } => "feature_not_supported",
        }
    }
}

impl fmt::Display for ValidationCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code())?;
        match self {
            Self::SliceUnknown { id } => write!(f, "{{id={id}}}"),
            Self::SliceGap { missing } => write!(f, "{{missing={missing:?}}}"),
            Self::DeferredMissing { missing } => write!(f, "{{missing={missing:?}}}"),
            Self::DeferredMarkedCancelled { id } => write!(f, "{{id={id}}}"),
            Self::NongoalUnspecified { id, spec } => write!(f, "{{id={id}, spec={spec}}}"),
            Self::EntrypointUnknownHandler { handler, known } => {
                write!(f, "{{handler={handler}, known={known:?}}}")
            }
            Self::TransportUndeclared { entrypoint } => write!(f, "{{entrypoint={entrypoint}}}"),
            Self::MasterKeyUnreadable { path } => write!(f, "{{path={path}}}"),
            Self::ProfileMismatch {
                profile,
                expected_k,
                actual_k,
            } => write!(
                f,
                "{{profile={profile}, expected_k={expected_k}, actual_k={actual_k}}}"
            ),
            Self::ProfileIncomplete { profile, missing } => {
                write!(f, "{{profile={profile}, missing={missing:?}}}")
            }
            Self::FeatureNotSupported { feature } => write!(f, "{{feature={feature}}}"),
            _ => Ok(()),
        }
    }
}

impl std::error::Error for ValidationCode {}
