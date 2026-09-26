//! Hybrid logical clock stamp.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum HlcCompareError {
    #[error("HlcCrossDomain")]
    CrossDomain,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HlcStamp {
    pub domain_id: String,
    pub physical_micros: u64,
    pub logical: u32,
    pub node_id: Uuid,
}

impl HlcStamp {
    /// Tick: physical = max(wall, last.physical); equal → logical += 1 else logical = 0.
    pub fn tick(&self, wall_micros: u64, node_id: Uuid) -> Self {
        let physical = wall_micros.max(self.physical_micros);
        let logical = if physical == self.physical_micros {
            self.logical.saturating_add(1)
        } else {
            0
        };
        Self {
            domain_id: self.domain_id.clone(),
            physical_micros: physical,
            logical,
            node_id,
        }
    }

    pub fn compare(&self, other: &Self) -> Result<std::cmp::Ordering, HlcCompareError> {
        if self.domain_id != other.domain_id {
            return Err(HlcCompareError::CrossDomain);
        }
        Ok(self
            .physical_micros
            .cmp(&other.physical_micros)
            .then(self.logical.cmp(&other.logical))
            .then(self.node_id.cmp(&other.node_id)))
    }

    /// Observe remote stamp; advance local so next tick stays monotonic.
    pub fn merge_observe(&self, remote: &Self, node_id: Uuid) -> Result<Self, HlcCompareError> {
        if self.domain_id != remote.domain_id {
            return Err(HlcCompareError::CrossDomain);
        }
        let physical = self.physical_micros.max(remote.physical_micros);
        let logical = if physical == self.physical_micros && physical == remote.physical_micros {
            self.logical.max(remote.logical)
        } else if physical == remote.physical_micros {
            remote.logical
        } else {
            self.logical
        };
        Ok(Self {
            domain_id: self.domain_id.clone(),
            physical_micros: physical,
            logical,
            node_id,
        })
    }
}

pub fn wall_micros() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_never_decreases() {
        let id = Uuid::new_v4();
        let mut s = HlcStamp {
            domain_id: "lab".into(),
            physical_micros: 100,
            logical: 0,
            node_id: id,
        };
        for wall in [50, 100, 100, 200] {
            let next = s.tick(wall, id);
            assert!(next.physical_micros >= s.physical_micros);
            if next.physical_micros == s.physical_micros {
                assert!(next.logical > s.logical);
            }
            s = next;
        }
    }

    #[test]
    fn cross_domain_compare_errors() {
        let a = HlcStamp {
            domain_id: "a".into(),
            physical_micros: 1,
            logical: 0,
            node_id: Uuid::new_v4(),
        };
        let b = HlcStamp {
            domain_id: "b".into(),
            physical_micros: 2,
            logical: 0,
            node_id: Uuid::new_v4(),
        };
        assert_eq!(a.compare(&b).unwrap_err(), HlcCompareError::CrossDomain);
    }
}
