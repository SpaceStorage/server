//! Release profile enumeration.

use crate::handlers::HandlerBuildSet;
use crate::types::TypeRequirement;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReleaseProfile {
    FirstBinary,
    /// Slice-6 gate: all eight protocol handlers at `015` HandlersComplete MUST.
    HandlersComplete,
    CompleteProduct,
}

impl ReleaseProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FirstBinary => "first-binary",
            Self::HandlersComplete => "handlers-complete",
            Self::CompleteProduct => "complete-product",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "first-binary" => Some(Self::FirstBinary),
            "handlers-complete" => Some(Self::HandlersComplete),
            "complete-product" => Some(Self::CompleteProduct),
            _ => None,
        }
    }

    pub fn handler_set(self) -> HandlerBuildSet {
        HandlerBuildSet::for_profile(self)
    }

    pub fn type_requirement(self) -> TypeRequirement {
        TypeRequirement::for_profile(self)
    }

    /// Expected implemented max `k` for this profile.
    pub fn expected_implemented_max(self) -> u8 {
        match self {
            Self::FirstBinary => 5,
            Self::HandlersComplete => 6,
            Self::CompleteProduct => 11,
        }
    }

    /// Map onto the 015 dialect profile name (`spacestorage-compat::DialectProfile`).
    pub fn dialect_profile_name(self) -> &'static str {
        match self {
            Self::FirstBinary => "first-binary",
            Self::HandlersComplete => "handlers-complete",
            Self::CompleteProduct => "complete-product",
        }
    }
}
