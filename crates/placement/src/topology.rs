//! Topology ladder: ordered subsequence of rack/az/region/continent/planet.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const RESERVED: &[&str] = &["rack", "az", "region", "continent", "planet"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TopologyLadder {
    pub keys: Vec<String>,
}

impl TopologyLadder {
    pub fn parse(keys: Vec<String>) -> Result<Self, String> {
        if keys.is_empty() {
            return Err("topology_ladder_empty".into());
        }
        let mut seen_idx = -1i32;
        for k in &keys {
            let Some(idx) = RESERVED.iter().position(|r| *r == k.as_str()) else {
                // Custom keys allowed after reserved subsequence.
                continue;
            };
            if idx as i32 <= seen_idx {
                return Err("topology_ladder_order".into());
            }
            seen_idx = idx as i32;
        }
        Ok(Self { keys })
    }

    pub fn first_binary_az() -> Self {
        Self {
            keys: vec!["az".into()],
        }
    }

    /// Finest key = first on the ladder (FR-020 default anti-affinity for RF≥2).
    pub fn finest_key(&self) -> Option<&str> {
        self.keys.first().map(|s| s.as_str())
    }

    pub fn fill_ok(&self, labels: &BTreeMap<String, String>) -> bool {
        self.keys.iter().all(|k| labels.contains_key(k))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeTopo {
    pub name: String,
    pub labels: BTreeMap<String, String>,
    pub quorum_domain: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TopologyView {
    pub ladder: Vec<String>,
    pub nodes: Vec<NodeTopo>,
}

impl TopologyView {
    pub fn domains_for_key(&self, key: &str) -> Vec<String> {
        let mut v: Vec<_> = self
            .nodes
            .iter()
            .filter_map(|n| n.labels.get(key).cloned())
            .collect();
        v.sort();
        v.dedup();
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn az_ladder_fill() {
        let ladder = TopologyLadder::first_binary_az();
        let mut labels = BTreeMap::new();
        assert!(!ladder.fill_ok(&labels));
        labels.insert("az".into(), "a".into());
        assert!(ladder.fill_ok(&labels));
    }
}
