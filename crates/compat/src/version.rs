//! Product version window N / N+1 (contracts/version-window.md).

use crate::error::CompatError;
use serde::{Deserialize, Serialize};

/// Product major version. First binary = 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ProductVersion(pub u16);

impl ProductVersion {
    /// Compiled-in first-binary product version.
    pub const FIRST_BINARY: Self = Self(1);

    pub fn get(self) -> u16 {
        self.0
    }

    /// Peers within the N/N+1 window.
    pub fn peers_ok(local: Self, peer: Self) -> bool {
        local.0.abs_diff(peer.0) <= 1
    }

    /// Refuse join when peer is outside the window.
    pub fn check_peer(local: Self, peer: Self) -> Result<(), CompatError> {
        if Self::peers_ok(local, peer) {
            Ok(())
        } else {
            Err(CompatError::ProductVersionWindow {
                local: local.0,
                peer: peer.0,
            })
        }
    }

    /// N node must not create a type introduced in a newer major.
    pub fn check_type_intro(
        local: Self,
        type_name: &str,
        introduced_in: Self,
    ) -> Result<(), CompatError> {
        if introduced_in.0 <= local.0 {
            Ok(())
        } else {
            Err(CompatError::TypeTooNew {
                type_name: type_name.to_string(),
                introduced_in: introduced_in.0,
                local: local.0,
            })
        }
    }

    /// N node must not write disk format newer than local.
    pub fn check_format(local: Self, format: Self) -> Result<(), CompatError> {
        if format.0 <= local.0 {
            Ok(())
        } else {
            Err(CompatError::FormatTooNew {
                have: format.0,
                need: local.0,
            })
        }
    }

    /// Config `product_version 0` is invalid.
    pub fn from_config(n: u16) -> Result<Self, CompatError> {
        if n == 0 {
            Err(CompatError::ProductVersionZero)
        } else {
            Ok(Self(n))
        }
    }
}

/// Free function alias matching the data-model signature.
pub fn peers_ok(local: ProductVersion, peer: ProductVersion) -> bool {
    ProductVersion::peers_ok(local, peer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window() {
        assert!(peers_ok(ProductVersion(1), ProductVersion(2)));
        assert!(peers_ok(ProductVersion(2), ProductVersion(1)));
        assert!(peers_ok(ProductVersion(1), ProductVersion(1)));
        assert!(!peers_ok(ProductVersion(1), ProductVersion(3)));
        assert!(ProductVersion::check_peer(ProductVersion(1), ProductVersion(3)).is_err());
    }

    #[test]
    fn zero_refused() {
        assert_eq!(
            ProductVersion::from_config(0).unwrap_err(),
            CompatError::ProductVersionZero
        );
    }
}
