//! Slice-7 voter migrate + exclusive-data helpers (`controlplane-ops`).

use crate::error::{ControlPlaneError, Result};
use crate::membership::{MembershipOp, VoterSet};
use std::collections::BTreeSet;
use uuid::Uuid;

/// Propose Replace / Grow / Shrink. Without `controlplane-ops` → `Slice7Required`.
pub fn propose_membership_op(
    vs: &VoterSet,
    op: MembershipOp,
    members: &BTreeSet<Uuid>,
) -> Result<VoterSet> {
    op.apply(vs, members)
}

/// When exclusive-data is requested on, refuse if any listed containers still live on voters.
pub fn check_exclusive_data_on(
    exclusive_on: bool,
    voter_nodes: &BTreeSet<Uuid>,
    tenant_replicas_on_voters: &[(Uuid, Uuid)], // (node, container)
) -> Result<()> {
    if !exclusive_on {
        return Ok(());
    }
    #[cfg(not(feature = "controlplane-ops"))]
    {
        let _ = (voter_nodes, tenant_replicas_on_voters);
        return Err(ControlPlaneError::Slice7Required {
            op: "controller_exclusive_data".into(),
        });
    }
    #[cfg(feature = "controlplane-ops")]
    {
        let blocked: Vec<(Uuid, Uuid)> = tenant_replicas_on_voters
            .iter()
            .copied()
            .filter(|(n, _)| voter_nodes.contains(n))
            .collect();
        if let Some((node, _)) = blocked.first() {
            return Err(ControlPlaneError::ExclusiveDataBlocked {
                node: *node,
                containers: blocked.iter().map(|(_, c)| *c).collect(),
            });
        }
        Ok(())
    }
}

/// Placement selector: voters excluded as tenant replica targets when exclusive-data on.
pub fn tenant_replica_excluded(node: Uuid, exclusive_data: bool, voters: &BTreeSet<Uuid>) -> bool {
    exclusive_data && voters.contains(&node)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::group::GroupId;

    #[test]
    fn exclusive_gated_without_ops() {
        let voters: BTreeSet<_> = [Uuid::from_u128(1)].into_iter().collect();
        let err = check_exclusive_data_on(true, &voters, &[]).unwrap_err();
        assert!(matches!(err, ControlPlaneError::Slice7Required { .. }));
    }

    #[test]
    fn first_binary_replace_gated() {
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        let vs = VoterSet::singleton(GroupId::Cluster, a).unwrap();
        let members: BTreeSet<_> = [a, b].into_iter().collect();
        let err = propose_membership_op(&vs, MembershipOp::Replace { from: a, to: b }, &members)
            .unwrap_err();
        assert!(matches!(err, ControlPlaneError::Slice7Required { .. }));
    }
}
