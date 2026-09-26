//! Named error codes for the compatibility ceiling (data-model §9).

use serde::{Deserialize, Serialize};
use std::fmt;

/// Stable string codes handlers / exec / config surface to clients and logs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompatError {
    CompatMustNot {
        protocol: String,
        verb: String,
    },
    SerializableNongoal,
    SnapshotUnsupported {
        type_name: String,
    },
    LimitExceeded {
        limit: String,
        current: u64,
        max: u64,
    },
    AdmissionRejected {
        limit: String,
        current: u64,
        max: u64,
    },
    ProductVersionWindow {
        local: u16,
        peer: u16,
    },
    FormatTooNew {
        have: u16,
        need: u16,
    },
    TypeTooNew {
        type_name: String,
        introduced_in: u16,
        local: u16,
    },
    LimitsZero {
        knob: String,
    },
    LimitsUnknownUnit,
    ProductVersionZero,
    CopyNotInProfile,
    BeginNotInProfile,
    CursorNotInProfile,
    AggNotInProfile,
}

impl CompatError {
    /// Normative code token (without parameter payload formatting).
    pub fn code(&self) -> &'static str {
        match self {
            Self::CompatMustNot { .. } => "compat_must_not",
            Self::SerializableNongoal => "serializable_nongoal",
            Self::SnapshotUnsupported { .. } => "snapshot_unsupported",
            Self::LimitExceeded { .. } => "limit_exceeded",
            Self::AdmissionRejected { .. } => "admission_rejected",
            Self::ProductVersionWindow { .. } => "product_version_window",
            Self::FormatTooNew { .. } => "format_too_new",
            Self::TypeTooNew { .. } => "type_too_new",
            Self::LimitsZero { .. } => "limits_zero",
            Self::LimitsUnknownUnit => "limits_unknown_unit",
            Self::ProductVersionZero => "product_version_zero",
            Self::CopyNotInProfile => "copy_not_in_profile",
            Self::BeginNotInProfile => "begin_not_in_profile",
            Self::CursorNotInProfile => "cursor_not_in_profile",
            Self::AggNotInProfile => "agg_not_in_profile",
        }
    }

    pub fn limit_exceeded(limit: impl Into<String>, current: u64, max: u64) -> Self {
        Self::LimitExceeded {
            limit: limit.into(),
            current,
            max,
        }
    }

    pub fn admission_rejected(limit: impl Into<String>, current: u64, max: u64) -> Self {
        Self::AdmissionRejected {
            limit: limit.into(),
            current,
            max,
        }
    }

    pub fn limits_zero(knob: impl Into<String>) -> Self {
        Self::LimitsZero {
            knob: knob.into(),
        }
    }
}

impl fmt::Display for CompatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CompatMustNot { protocol, verb } => {
                write!(f, "compat_must_not{{protocol:{protocol},verb:{verb}}}")
            }
            Self::SerializableNongoal => write!(f, "serializable_nongoal"),
            Self::SnapshotUnsupported { type_name } => {
                write!(f, "snapshot_unsupported{{type:{type_name}}}")
            }
            Self::LimitExceeded {
                limit,
                current,
                max,
            } => write!(f, "limit_exceeded{{limit:{limit},current:{current},max:{max}}}"),
            Self::AdmissionRejected {
                limit,
                current,
                max,
            } => write!(
                f,
                "admission_rejected{{limit:{limit},current:{current},max:{max}}}"
            ),
            Self::ProductVersionWindow { local, peer } => {
                write!(f, "product_version_window{{local:{local},peer:{peer}}}")
            }
            Self::FormatTooNew { have, need } => {
                write!(f, "format_too_new{{have:{have},need:{need}}}")
            }
            Self::TypeTooNew {
                type_name,
                introduced_in,
                local,
            } => write!(
                f,
                "type_too_new{{type:{type_name},introduced_in:{introduced_in},local:{local}}}"
            ),
            Self::LimitsZero { knob } => write!(f, "limits_zero{{knob:{knob}}}"),
            Self::LimitsUnknownUnit => write!(f, "limits_unknown_unit"),
            Self::ProductVersionZero => write!(f, "product_version_zero"),
            Self::CopyNotInProfile => write!(f, "copy_not_in_profile"),
            Self::BeginNotInProfile => write!(f, "begin_not_in_profile"),
            Self::CursorNotInProfile => write!(f, "cursor_not_in_profile"),
            Self::AggNotInProfile => write!(f, "agg_not_in_profile"),
        }
    }
}

impl std::error::Error for CompatError {}
