//! Operator drain / undrain (process stays up; ≠ stop).

use crate::error::{MembershipError, Result};
use crate::events::{MemberStatus, MembershipEvent};
use crate::view::MembershipView;
use uuid::Uuid;

pub fn drain(view: &mut MembershipView, node_id: Uuid) -> Result<MembershipEvent> {
    let Some(m) = view.members.get_mut(&node_id) else {
        return Err(MembershipError::NotMember);
    };
    m.status = MemberStatus::Draining;
    Ok(MembershipEvent::Drain { node_id })
}

pub fn undrain(view: &mut MembershipView, node_id: Uuid) -> Result<MembershipEvent> {
    let Some(m) = view.members.get_mut(&node_id) else {
        return Err(MembershipError::NotMember);
    };
    m.status = MemberStatus::Ready;
    Ok(MembershipEvent::Undrain { node_id })
}
