//! Dialect profiles — FirstBinary ⊂ HandlersComplete ⊂ CompleteProduct.
//! Never brand a profile as "v1".

use serde::{Deserialize, Serialize};

/// Compatibility dialect compiled from the release profile / Cargo features.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DialectProfile {
    FirstBinary,
    HandlersComplete,
    CompleteProduct,
}

impl DialectProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FirstBinary => "first-binary",
            Self::HandlersComplete => "handlers-complete",
            Self::CompleteProduct => "complete-product",
        }
    }

    /// Subset relation: `self` is a subset of (or equal to) `other`.
    pub fn is_subset_of(self, other: Self) -> bool {
        self <= other
    }

    /// Handlers present for this profile (protocol entrypoints only).
    pub fn protocol_handlers(self) -> &'static [&'static str] {
        match self {
            Self::FirstBinary => &["postgresql", "redis"],
            Self::HandlersComplete | Self::CompleteProduct => &[
                "postgresql",
                "redis",
                "cassandra",
                "elasticsearch",
                "clickhouse",
                "clickhouse-http",
                "s3",
                "webdav",
            ],
        }
    }

    /// True when an entrypoint handler name is allowed in this profile.
    pub fn allows_handler(self, name: &str) -> bool {
        match name {
            "admin" | "admin-http" | "internode" | "replication" => true,
            other => self.protocol_handlers().contains(&other),
        }
    }
}

/// Default / compile-time profile for the first-binary build (slices 1–5).
///
/// Mapping (016 / release-profile):
/// - `first-binary` → [`DialectProfile::FirstBinary`]
/// - `handlers-complete` → [`DialectProfile::HandlersComplete`]
/// - `query-distributed` / complete-product → [`DialectProfile::CompleteProduct`]
pub fn active_profile() -> DialectProfile {
    // Feature flags are owned by the binary / release-profile crate; this
    // library defaults to FirstBinary. Callers override via explicit profile.
    DialectProfile::FirstBinary
}

/// Map a release-profile string (`first-binary`, `handlers-complete`,
/// `complete-product`, `query-distributed`) onto a dialect profile.
pub fn profile_from_release(name: &str) -> Option<DialectProfile> {
    match name {
        "first-binary" => Some(DialectProfile::FirstBinary),
        "handlers-complete" => Some(DialectProfile::HandlersComplete),
        "complete-product" | "query-distributed" => Some(DialectProfile::CompleteProduct),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subset_order() {
        assert!(DialectProfile::FirstBinary.is_subset_of(DialectProfile::HandlersComplete));
        assert!(DialectProfile::HandlersComplete.is_subset_of(DialectProfile::CompleteProduct));
        assert!(!DialectProfile::CompleteProduct.is_subset_of(DialectProfile::FirstBinary));
    }

    #[test]
    fn never_named_v1() {
        for p in [
            DialectProfile::FirstBinary,
            DialectProfile::HandlersComplete,
            DialectProfile::CompleteProduct,
        ] {
            assert!(!p.as_str().contains('v'));
            assert_ne!(p.as_str(), "v1");
        }
    }

    #[test]
    fn fb_forbids_elasticsearch() {
        assert!(!DialectProfile::FirstBinary.allows_handler("elasticsearch"));
        assert!(DialectProfile::FirstBinary.allows_handler("postgresql"));
        assert!(DialectProfile::HandlersComplete.allows_handler("elasticsearch"));
    }
}
