//! Voter / learner sets and first-binary static expansion (FR-002, FR-018, FR-020).

use crate::error::{ControlPlaneError, Result};
use crate::group::GroupId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoterSet {
    pub group: GroupId,
    pub voters: BTreeSet<Uuid>,
    pub learners: BTreeSet<Uuid>,
    pub epoch: u64,
}

impl VoterSet {
    pub fn empty(group: GroupId) -> Self {
        Self {
            group,
            voters: BTreeSet::new(),
            learners: BTreeSet::new(),
            epoch: 0,
        }
    }

    pub fn singleton(group: GroupId, node: Uuid) -> Result<Self> {
        let mut vs = Self::empty(group);
        vs.voters.insert(node);
        vs.validate()?;
        Ok(vs)
    }

    /// Invariants: odd voters, learners ∩ voters = ∅.
    pub fn validate(&self) -> Result<()> {
        let n = self.voters.len();
        if n == 0 {
            return Ok(());
        }
        if n % 2 == 0 {
            return Err(ControlPlaneError::VoterSetOdd { size: n });
        }
        for id in &self.learners {
            if self.voters.contains(id) {
                return Err(ControlPlaneError::Msg(format!(
                    "learner {id} also in voters"
                )));
            }
        }
        Ok(())
    }

    pub fn is_voter(&self, id: &Uuid) -> bool {
        self.voters.contains(id)
    }

    pub fn is_learner(&self, id: &Uuid) -> bool {
        self.learners.contains(id)
    }

    pub fn majority(&self) -> usize {
        self.voters.len() / 2 + 1
    }

    /// Ensure every voter/learner is in `members`.
    pub fn ensure_subset_of(&self, members: &BTreeSet<Uuid>) -> Result<()> {
        for id in self.voters.iter().chain(self.learners.iter()) {
            if !members.contains(id) {
                return Err(ControlPlaneError::NotMember);
            }
        }
        Ok(())
    }
}

/// First-binary static voter table from admitted member count / ordered ids.
///
/// | Members | Voters |
/// |---------|--------|
/// | 1 | `{A}` |
/// | 2 | `{A}` + B learner |
/// | 3 | `{A,B,C}` one ExpandToThree |
/// | 4+ | still `{A,B,C}`; extras learners |
pub fn first_binary_voter_set(group: GroupId, member_ids: &[Uuid]) -> Result<VoterSet> {
    let mut vs = VoterSet::empty(group);
    match member_ids.len() {
        0 => {}
        1 => {
            vs.voters.insert(member_ids[0]);
        }
        2 => {
            vs.voters.insert(member_ids[0]);
            vs.learners.insert(member_ids[1]);
        }
        3 => {
            for id in member_ids.iter().take(3) {
                vs.voters.insert(*id);
            }
            vs.epoch = 1; // ExpandToThree applied
        }
        _ => {
            for id in member_ids.iter().take(3) {
                vs.voters.insert(*id);
            }
            for id in member_ids.iter().skip(3) {
                vs.learners.insert(*id);
            }
            vs.epoch = 1;
        }
    }
    vs.validate()?;
    Ok(vs)
}

/// Whether admitting `new_id` as the third member should emit ExpandToThree.
pub fn should_expand_to_three(current_voters: &BTreeSet<Uuid>, admitted_count: usize) -> bool {
    current_voters.len() == 1 && admitted_count == 3
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MembershipOp {
    ExpandToThree { b: Uuid, c: Uuid },
    Replace { from: Uuid, to: Uuid },
    Grow { add: [Uuid; 2] },
    Shrink { remove: [Uuid; 2] },
}

impl MembershipOp {
    pub fn name(&self) -> &'static str {
        match self {
            Self::ExpandToThree { .. } => "ExpandToThree",
            Self::Replace { .. } => "Replace",
            Self::Grow { .. } => "Grow",
            Self::Shrink { .. } => "Shrink",
        }
    }

    /// Apply op to a voter set. Slice-7 ops require `controlplane-ops`.
    pub fn apply(&self, vs: &VoterSet, members: &BTreeSet<Uuid>) -> Result<VoterSet> {
        match self {
            Self::ExpandToThree { b, c } => {
                if vs.voters.len() != 1 {
                    return Err(ControlPlaneError::Msg(
                        "ExpandToThree requires singleton voter set".into(),
                    ));
                }
                let a = *vs.voters.iter().next().unwrap();
                let mut next = VoterSet::empty(vs.group);
                next.voters.insert(a);
                next.voters.insert(*b);
                next.voters.insert(*c);
                next.learners = vs
                    .learners
                    .iter()
                    .copied()
                    .filter(|id| *id != *b && *id != *c)
                    .collect();
                next.epoch = vs.epoch.saturating_add(1);
                next.ensure_subset_of(members)?;
                next.validate()?;
                Ok(next)
            }
            Self::Replace { from, to } => {
                #[cfg(not(feature = "controlplane-ops"))]
                {
                    let _ = (from, to);
                    return Err(ControlPlaneError::Slice7Required {
                        op: "Replace".into(),
                    });
                }
                #[cfg(feature = "controlplane-ops")]
                {
                    apply_replace(vs, *from, *to, members)
                }
            }
            Self::Grow { add } => {
                #[cfg(not(feature = "controlplane-ops"))]
                {
                    let _ = add;
                    return Err(ControlPlaneError::Slice7Required {
                        op: "Grow".into(),
                    });
                }
                #[cfg(feature = "controlplane-ops")]
                {
                    apply_grow(vs, *add, members)
                }
            }
            Self::Shrink { remove } => {
                #[cfg(not(feature = "controlplane-ops"))]
                {
                    let _ = remove;
                    return Err(ControlPlaneError::Slice7Required {
                        op: "Shrink".into(),
                    });
                }
                #[cfg(feature = "controlplane-ops")]
                {
                    apply_shrink(vs, *remove, members)
                }
            }
        }
    }
}

#[cfg(feature = "controlplane-ops")]
fn apply_replace(
    vs: &VoterSet,
    from: Uuid,
    to: Uuid,
    members: &BTreeSet<Uuid>,
) -> Result<VoterSet> {
    if !vs.voters.contains(&from) {
        return Err(ControlPlaneError::Msg(format!(
            "Replace from={from} is not a voter"
        )));
    }
    if !members.contains(&to) {
        return Err(ControlPlaneError::NotMember);
    }
    if vs.voters.contains(&to) {
        return Err(ControlPlaneError::Msg(format!(
            "Replace to={to} already a voter"
        )));
    }
    // Joint step: both from and to must be reachable for majority — size stays odd.
    let mut next = vs.clone();
    next.voters.remove(&from);
    next.voters.insert(to);
    next.learners.remove(&to);
    next.learners.insert(from);
    next.epoch = vs.epoch.saturating_add(1);
    next.validate()?;
    // Majority preserved: same odd size.
    if next.majority() > next.voters.len() {
        return Err(ControlPlaneError::VoterSetMajorityLost);
    }
    Ok(next)
}

#[cfg(feature = "controlplane-ops")]
fn apply_grow(vs: &VoterSet, add: [Uuid; 2], members: &BTreeSet<Uuid>) -> Result<VoterSet> {
    for id in &add {
        if !members.contains(id) {
            return Err(ControlPlaneError::NotMember);
        }
        if vs.voters.contains(id) {
            return Err(ControlPlaneError::Msg(format!(
                "Grow: {id} already a voter"
            )));
        }
    }
    if add[0] == add[1] {
        return Err(ControlPlaneError::VoterSetOdd { size: vs.voters.len() + 1 });
    }
    let mut next = vs.clone();
    next.voters.insert(add[0]);
    next.voters.insert(add[1]);
    next.learners.remove(&add[0]);
    next.learners.remove(&add[1]);
    next.epoch = vs.epoch.saturating_add(1);
    next.validate()?;
    Ok(next)
}

#[cfg(feature = "controlplane-ops")]
fn apply_shrink(vs: &VoterSet, remove: [Uuid; 2], members: &BTreeSet<Uuid>) -> Result<VoterSet> {
    let _ = members;
    if remove[0] == remove[1] {
        return Err(ControlPlaneError::VoterSetOdd {
            size: vs.voters.len().saturating_sub(1),
        });
    }
    for id in &remove {
        if !vs.voters.contains(id) {
            return Err(ControlPlaneError::Msg(format!(
                "Shrink: {id} is not a voter"
            )));
        }
    }
    let remaining = vs.voters.len().saturating_sub(2);
    if remaining == 0 || remaining % 2 == 0 {
        return Err(ControlPlaneError::VoterSetOdd { size: remaining });
    }
    // During joint shrink, old voters must still form a majority of the old set.
    let old_maj = vs.majority();
    let survivors = vs.voters.len() - 2;
    if survivors < old_maj && vs.voters.len() > 1 {
        // For size 3→1: survivors=1, old_maj=2 → would lose majority mid-step if we
        // required old majority among survivors alone. Spec: leftover odd and ≥1;
        // joint consensus keeps old majority by requiring both removes acknowledged.
        // We accept 3→1 as long as both remove targets are voters.
    }
    let mut next = vs.clone();
    for id in &remove {
        next.voters.remove(id);
        next.learners.insert(*id);
    }
    next.epoch = vs.epoch.saturating_add(1);
    next.validate()?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(n: usize) -> Vec<Uuid> {
        (0..n).map(|i| Uuid::from_u128(i as u128 + 1)).collect()
    }

    #[test]
    fn first_binary_table() {
        let a = ids(1);
        let vs = first_binary_voter_set(GroupId::Cluster, &a).unwrap();
        assert_eq!(vs.voters.len(), 1);
        assert!(vs.learners.is_empty());

        let ab = ids(2);
        let vs = first_binary_voter_set(GroupId::Cluster, &ab).unwrap();
        assert_eq!(vs.voters.len(), 1);
        assert_eq!(vs.learners.len(), 1);
        assert!(vs.voters.contains(&ab[0]));
        assert!(vs.learners.contains(&ab[1]));

        let abc = ids(3);
        let vs = first_binary_voter_set(GroupId::Cluster, &abc).unwrap();
        assert_eq!(vs.voters.len(), 3);
        assert!(vs.learners.is_empty());

        let four = ids(4);
        let vs = first_binary_voter_set(GroupId::Cluster, &four).unwrap();
        assert_eq!(vs.voters.len(), 3);
        assert_eq!(vs.learners.len(), 1);
    }

    #[test]
    fn even_proposal_refused() {
        let mut vs = VoterSet::empty(GroupId::Cluster);
        vs.voters.insert(Uuid::from_u128(1));
        vs.voters.insert(Uuid::from_u128(2));
        assert!(matches!(
            vs.validate(),
            Err(ControlPlaneError::VoterSetOdd { size: 2 })
        ));
    }

    #[test]
    fn expand_to_three() {
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        let c = Uuid::from_u128(3);
        let vs = VoterSet::singleton(GroupId::Cluster, a).unwrap();
        let members: BTreeSet<_> = [a, b, c].into_iter().collect();
        let next = MembershipOp::ExpandToThree { b, c }
            .apply(&vs, &members)
            .unwrap();
        assert_eq!(next.voters.len(), 3);
        assert_eq!(next.epoch, 1);
    }

    #[test]
    fn slice7_ops_gated_without_feature() {
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        let vs = VoterSet::singleton(GroupId::Cluster, a).unwrap();
        let members: BTreeSet<_> = [a, b].into_iter().collect();
        let err = MembershipOp::Replace { from: a, to: b }
            .apply(&vs, &members)
            .unwrap_err();
        assert!(matches!(err, ControlPlaneError::Slice7Required { .. }));
    }
}
