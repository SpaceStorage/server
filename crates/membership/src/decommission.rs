//! Decommission: remove member + retire identity; refuse last member.

use crate::error::{MembershipError, Result};
use crate::events::{MemberStatus, MembershipEvent};
use crate::view::MembershipView;
use uuid::Uuid;

pub fn decommission(
    view: &mut MembershipView,
    node_id: Uuid,
    accept_data_loss: bool,
    blockers: &[String],
) -> Result<Vec<MembershipEvent>> {
    if view.members.len() <= 1 {
        return Err(MembershipError::LastMember);
    }
    let Some(m) = view.members.get(&node_id).cloned() else {
        return Err(MembershipError::NotMember);
    };
    if matches!(m.status, MemberStatus::Ready) {
        // Reachable ready members must drain first (simplified FB).
        return Err(MembershipError::InvalidState("must_drain".into()));
    }
    if !blockers.is_empty() && !accept_data_loss {
        return Err(MembershipError::DecommissionBlocked);
    }
    let former_name = m.node_name;
    view.members.remove(&node_id);
    view.retired.insert(node_id);
    view.recompute_voters();
    Ok(vec![
        MembershipEvent::RemoveMember { node_id },
        MembershipEvent::RetireIdentity {
            node_id,
            former_name,
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::MemberStatus;
    use crate::view::MemberRecord;
    use std::collections::BTreeMap;

    #[test]
    fn last_member_refused() {
        let mut view = MembershipView::empty(Uuid::new_v4(), "lab", 1);
        let id = Uuid::new_v4();
        view.members.insert(
            id,
            MemberRecord {
                node_id: id,
                node_name: "n1".into(),
                status: MemberStatus::Draining,
                incarnation: 1,
                quorum_domain: "lab".into(),
                voter: true,
                labels: BTreeMap::new(),
            },
        );
        assert_eq!(
            decommission(&mut view, id, true, &[]).unwrap_err(),
            MembershipError::LastMember
        );
    }
}
