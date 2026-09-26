//! Release profile enumeration.

use crate::handlers::HandlerBuildSet;
use crate::types::TypeRequirement;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReleaseProfile {
    FirstBinary,
    CompleteProduct,
}

impl ReleaseProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FirstBinary => "first-binary",
            Self::CompleteProduct => "complete-product",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "first-binary" => Some(Self::FirstBinary),
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
            Self::CompleteProduct => 11,
        }
    }

    /// Map onto the 015 dialect profile name (`spacestorage-compat::DialectProfile`).
    ///
    /// - `first-binary` → FirstBinary
    /// - `complete-product` → CompleteProduct
    /// - HandlersComplete is selected via the `handlers-complete` Cargo feature /
    ///   `spacestorage_compat::profile_from_release("handlers-complete")`, not this enum yet.
    pub fn dialect_profile_name(self) -> &'static str {
        match self {
            Self::FirstBinary => "first-binary",
            Self::CompleteProduct => "complete-product",
        }
    }
}
