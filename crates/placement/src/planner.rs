//! Deterministic greedy planner with anti-affinity by ladder key.

use crate::error::PlacementError;
use crate::topology::NodeTopo;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct PlanRequest {
    pub container: String,
    pub replication_factor: u32,
    /// Explicit anti-affinity keys. When empty and RF≥2, defaults to the
    /// **finest** key on [`Self::topology_ladder`] (FR-020).
    pub anti_affinity_keys: Vec<String>,
    /// Cluster topology ladder (ordered finest→coarsest). Required to resolve
    /// the RF≥2 default when `anti_affinity_keys` is unset.
    pub topology_ladder: Vec<String>,
    pub candidates: Vec<NodeTopo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlacementPlan {
    pub targets: Vec<String>,
    /// Keys actually enforced for this plan (after RF≥2 default resolution).
    pub anti_affinity_keys: Vec<String>,
}

/// Finest ladder key = first entry (usually `az` or `rack` on FB `[az]`).
pub fn finest_ladder_key(ladder: &[String]) -> Option<&str> {
    ladder.first().map(|s| s.as_str())
}

/// Resolve anti-affinity keys: explicit list wins; else RF≥2 → finest ladder key.
pub fn resolve_anti_affinity_keys(
    replication_factor: u32,
    anti_affinity_keys: &[String],
    topology_ladder: &[String],
) -> Result<Vec<String>, PlacementError> {
    if !anti_affinity_keys.is_empty() {
        return Ok(anti_affinity_keys.to_vec());
    }
    if replication_factor < 2 {
        return Ok(Vec::new());
    }
    match finest_ladder_key(topology_ladder) {
        Some(k) => Ok(vec![k.to_string()]),
        None => Err(PlacementError::PlacementUnsatisfiable {
            container: String::new(),
            constraint: "anti_affinity_default".into(),
            hint: "RF≥2 requires topology_ladder to default anti-affinity to finest key"
                .into(),
        }),
    }
}

/// Greedy: exclude used anti-affinity values; tie-break by node name.
pub fn plan_replicas(req: &PlanRequest) -> Result<PlacementPlan, PlacementError> {
    if req.replication_factor == 0 {
        return Err(PlacementError::PlacementUnsatisfiable {
            container: req.container.clone(),
            constraint: "rf".into(),
            hint: "replication_factor must be >= 1".into(),
        });
    }
    let keys = match resolve_anti_affinity_keys(
        req.replication_factor,
        &req.anti_affinity_keys,
        &req.topology_ladder,
    ) {
        Ok(k) => k,
        Err(PlacementError::PlacementUnsatisfiable {
            constraint,
            hint,
            ..
        }) => {
            return Err(PlacementError::PlacementUnsatisfiable {
                container: req.container.clone(),
                constraint,
                hint,
            });
        }
        Err(e) => return Err(e),
    };

    let mut used: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for k in &keys {
        used.insert(k.clone(), BTreeSet::new());
    }
    let mut sorted = req.candidates.clone();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));

    let mut targets = Vec::new();
    for node in &sorted {
        if targets.len() as u32 >= req.replication_factor {
            break;
        }
        let mut ok = true;
        for k in &keys {
            let Some(val) = node.labels.get(k) else {
                ok = false;
                break;
            };
            if used.get(k).map(|s| s.contains(val)).unwrap_or(false) {
                ok = false;
                break;
            }
        }
        if !ok {
            continue;
        }
        for k in &keys {
            if let Some(val) = node.labels.get(k) {
                used.get_mut(k).unwrap().insert(val.clone());
            }
        }
        targets.push(node.name.clone());
    }
    if (targets.len() as u32) < req.replication_factor {
        return Err(PlacementError::PlacementUnsatisfiable {
            container: req.container.clone(),
            constraint: format!("anti_affinity={keys:?}"),
            hint: format!(
                "need {} distinct placements; available {}",
                req.replication_factor,
                targets.len()
            ),
        });
    }
    Ok(PlacementPlan {
        targets,
        anti_affinity_keys: keys,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn node(name: &str, az: &str) -> NodeTopo {
        NodeTopo {
            name: name.into(),
            labels: BTreeMap::from([("az".into(), az.into())]),
            quorum_domain: "lab".into(),
        }
    }

    fn node_rack_az(name: &str, rack: &str, az: &str) -> NodeTopo {
        NodeTopo {
            name: name.into(),
            labels: BTreeMap::from([
                ("rack".into(), rack.into()),
                ("az".into(), az.into()),
            ]),
            quorum_domain: "lab".into(),
        }
    }

    #[test]
    fn rf3_az_anti_affinity() {
        let plan = plan_replicas(&PlanRequest {
            container: "c".into(),
            replication_factor: 3,
            anti_affinity_keys: vec!["az".into()],
            topology_ladder: vec!["az".into()],
            candidates: vec![node("n1", "a"), node("n2", "b"), node("n3", "c")],
        })
        .unwrap();
        assert_eq!(plan.targets, vec!["n1", "n2", "n3"]);
    }

    #[test]
    fn unsatisfiable_same_az() {
        let err = plan_replicas(&PlanRequest {
            container: "c".into(),
            replication_factor: 3,
            anti_affinity_keys: vec!["az".into()],
            topology_ladder: vec!["az".into()],
            candidates: vec![node("n1", "a"), node("n2", "a"), node("n3", "a")],
        })
        .unwrap_err();
        assert!(matches!(err, PlacementError::PlacementUnsatisfiable { .. }));
    }

    #[test]
    fn empty_keys_rf3_defaults_finest_az_refuses_same_az() {
        // T096 CRITICAL: empty anti_affinity_keys MUST NOT place RF=3 on same az.
        let err = plan_replicas(&PlanRequest {
            container: "c".into(),
            replication_factor: 3,
            anti_affinity_keys: vec![],
            topology_ladder: vec!["az".into()],
            candidates: vec![node("n1", "a"), node("n2", "a"), node("n3", "a")],
        })
        .unwrap_err();
        assert!(matches!(err, PlacementError::PlacementUnsatisfiable { .. }));
    }

    #[test]
    fn empty_keys_rf3_defaults_finest_az_distinct_ok() {
        let plan = plan_replicas(&PlanRequest {
            container: "c".into(),
            replication_factor: 3,
            anti_affinity_keys: vec![],
            topology_ladder: vec!["az".into()],
            candidates: vec![node("n1", "a"), node("n2", "b"), node("n3", "c")],
        })
        .unwrap();
        assert_eq!(plan.anti_affinity_keys, vec!["az"]);
        assert_eq!(plan.targets, vec!["n1", "n2", "n3"]);
    }

    #[test]
    fn empty_keys_finest_is_first_ladder_key() {
        // Ladder [rack, az]: finest = rack — two nodes same rack refuse RF=2.
        let err = plan_replicas(&PlanRequest {
            container: "c".into(),
            replication_factor: 2,
            anti_affinity_keys: vec![],
            topology_ladder: vec!["rack".into(), "az".into()],
            candidates: vec![
                node_rack_az("n1", "r1", "a"),
                node_rack_az("n2", "r1", "b"),
            ],
        })
        .unwrap_err();
        assert!(matches!(err, PlacementError::PlacementUnsatisfiable { .. }));

        let plan = plan_replicas(&PlanRequest {
            container: "c".into(),
            replication_factor: 2,
            anti_affinity_keys: vec![],
            topology_ladder: vec!["rack".into(), "az".into()],
            candidates: vec![
                node_rack_az("n1", "r1", "a"),
                node_rack_az("n2", "r2", "a"),
            ],
        })
        .unwrap();
        assert_eq!(plan.anti_affinity_keys, vec!["rack"]);
        assert_eq!(plan.targets, vec!["n1", "n2"]);
    }

    #[test]
    fn rf1_empty_keys_no_anti_affinity() {
        let plan = plan_replicas(&PlanRequest {
            container: "c".into(),
            replication_factor: 1,
            anti_affinity_keys: vec![],
            topology_ladder: vec!["az".into()],
            candidates: vec![node("n1", "a"), node("n2", "a")],
        })
        .unwrap();
        assert!(plan.anti_affinity_keys.is_empty());
        assert_eq!(plan.targets, vec!["n1"]);
    }
}
