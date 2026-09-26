//! Replace FD-unavailable member; fence incarnation.

use crate::error::{MembershipError, Result};
use crate::events::{MemberStatus, MembershipEvent};
use crate::view::MembershipView;
use uuid::Uuid;

pub fn replace(
    view: &mut MembershipView,
    node_id: Uuid,
    fd_unavailable: bool,
) -> Result<(u64, MembershipEvent)> {
    if view.retired.contains(&node_id) {
        return Err(MembershipError::RetiredIdentity);
    }
    let Some(m) = view.members.get_mut(&node_id) else {
        return Err(MembershipError::NotMember);
    };
    if !fd_unavailable {
        if matches!(m.status, MemberStatus::Ready | MemberStatus::Draining) {
            return Err(MembershipError::LiveReplace);
        }
    }
    m.incarnation = m.incarnation.saturating_add(1);
    let inc = m.incarnation;
    Ok((inc, MembershipEvent::ReplaceMember {
        node_id,
        incarnation: inc,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::MemberRecord;
    use std::collections::BTreeMap;

    #[test]
    fn live_replace_refused() {
        let mut view = MembershipView::empty(Uuid::new_v4(), "lab", 1);
        let id = Uuid::new_v4();
        view.members.insert(
            id,
            MemberRecord {
                node_id: id,
                node_name: "n1".into(),
                status: MemberStatus::Ready,
                incarnation: 1,
                quorum_domain: "lab".into(),
                voter: true,
                labels: BTreeMap::new(),
            },
        );
        assert_eq!(
            replace(&mut view, id, false).unwrap_err(),
            MembershipError::LiveReplace
        );
    }

    #[test]
    fn unavailable_increments_incarnation() {
        let mut view = MembershipView::empty(Uuid::new_v4(), "lab", 1);
        let id = Uuid::new_v4();
        view.members.insert(
            id,
            MemberRecord {
                node_id: id,
                node_name: "n1".into(),
                status: MemberStatus::Ready,
                incarnation: 1,
                quorum_domain: "lab".into(),
                voter: true,
                labels: BTreeMap::new(),
            },
        );
        let (inc, _) = replace(&mut view, id, true).unwrap();
        assert_eq!(inc, 2);
    }
}
