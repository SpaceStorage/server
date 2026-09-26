//! Leadership leases with epoch fence (FR-006). Leaderless types MUST NOT take a lease.

use crate::error::{ControlPlaneError, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LeadershipLease {
    pub container_id: Uuid,
    pub holder: Uuid,
    pub epoch: u64,
    pub granted_index: u64,
}

/// Types that are leaderless in first binary / product default.
pub fn is_leaderless_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "K/V Store" | "Relational Table" | "Document Store" | "kv" | "sql" | "document"
    )
}

pub fn grant(
    container_id: Uuid,
    holder: Uuid,
    previous: Option<&LeadershipLease>,
    granted_index: u64,
    type_name: &str,
) -> Result<LeadershipLease> {
    if is_leaderless_type(type_name) {
        return Err(ControlPlaneError::LeaseForbidden {
            type_name: type_name.into(),
        });
    }
    let epoch = previous.map(|p| p.epoch.saturating_add(1)).unwrap_or(1);
    Ok(LeadershipLease {
        container_id,
        holder,
        epoch,
        granted_index,
    })
}

pub fn check_epoch(presented: u64, current: &LeadershipLease) -> Result<()> {
    if presented != current.epoch {
        return Err(ControlPlaneError::StaleEpoch {
            have: presented,
            need: current.epoch,
        });
    }
    Ok(())
}

pub fn require_lease(
    leases: &std::collections::BTreeMap<Uuid, LeadershipLease>,
    container_id: Uuid,
) -> Result<&LeadershipLease> {
    leases
        .get(&container_id)
        .ok_or(ControlPlaneError::LeaseNotGranted {
            container: container_id,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaderless_forbidden() {
        let err = grant(Uuid::nil(), Uuid::from_u128(1), None, 1, "K/V Store").unwrap_err();
        assert!(matches!(err, ControlPlaneError::LeaseForbidden { .. }));
    }

    #[test]
    fn epoch_bump_on_steal() {
        let c = Uuid::from_u128(9);
        let first = grant(c, Uuid::from_u128(1), None, 1, "Log Stream").unwrap();
        assert_eq!(first.epoch, 1);
        let second = grant(c, Uuid::from_u128(2), Some(&first), 2, "Log Stream").unwrap();
        assert_eq!(second.epoch, 2);
        assert!(check_epoch(1, &second).is_err());
        assert!(check_epoch(2, &second).is_ok());
    }
}
