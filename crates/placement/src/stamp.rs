//! VersionStamp HLC for in-domain LWW (re-export shape; clocks crate owns DomainClock).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VersionStamp {
    pub physical_micros: u64,
    pub logical: u32,
    pub node_id: Uuid,
}

impl VersionStamp {
    pub fn cmp_lww(&self, other: &Self) -> std::cmp::Ordering {
        self.physical_micros
            .cmp(&other.physical_micros)
            .then(self.logical.cmp(&other.logical))
            .then(self.node_id.cmp(&other.node_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lww_order() {
        let a = VersionStamp {
            physical_micros: 1,
            logical: 0,
            node_id: Uuid::nil(),
        };
        let b = VersionStamp {
            physical_micros: 2,
            logical: 0,
            node_id: Uuid::nil(),
        };
        assert_eq!(a.cmp_lww(&b), std::cmp::Ordering::Less);
    }
}
