//! Rolling upgrade helpers (015 N/N+1 polish).

use crate::error::CompatError;
use crate::version::ProductVersion;

/// Result of evaluating a peer join against the local product version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpgradeJoin {
    Accept,
    Refuse { local: u16, peer: u16 },
}

/// Mixed-version cluster: accept N↔N+1; refuse N+2 (and beyond).
pub fn evaluate_peer_join(local: ProductVersion, peer: ProductVersion) -> UpgradeJoin {
    if ProductVersion::peers_ok(local, peer) {
        UpgradeJoin::Accept
    } else {
        UpgradeJoin::Refuse {
            local: local.0,
            peer: peer.0,
        }
    }
}

/// N must not create a type introduced in a newer major.
pub fn refuse_type_too_new(
    local: ProductVersion,
    type_name: &str,
    introduced_in: ProductVersion,
) -> Result<(), CompatError> {
    ProductVersion::check_type_intro(local, type_name, introduced_in)
}

/// N must not write a disk format newer than local.
pub fn refuse_format_too_new(
    local: ProductVersion,
    format: ProductVersion,
) -> Result<(), CompatError> {
    ProductVersion::check_format(local, format)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn n_n1_ok_n2_refused() {
        assert_eq!(
            evaluate_peer_join(ProductVersion(1), ProductVersion(2)),
            UpgradeJoin::Accept
        );
        assert_eq!(
            evaluate_peer_join(ProductVersion(1), ProductVersion(3)),
            UpgradeJoin::Refuse { local: 1, peer: 3 }
        );
    }

    #[test]
    fn type_and_format_gates() {
        assert!(refuse_type_too_new(ProductVersion(1), "hash_table", ProductVersion(1)).is_ok());
        assert!(refuse_type_too_new(ProductVersion(1), "future", ProductVersion(2)).is_err());
        assert!(refuse_format_too_new(ProductVersion(1), ProductVersion(2)).is_err());
    }
}
