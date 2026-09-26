//! Follower apply-in-order (no independent source log).

use crate::source_log::SourceLogPosition;

#[derive(Debug, Default)]
pub struct FollowerApply {
    pub applied: SourceLogPosition,
}

impl FollowerApply {
    pub fn can_apply(&self, next: SourceLogPosition) -> bool {
        if next.epoch != self.applied.epoch && self.applied.position == 0 && self.applied.epoch == 0
        {
            return true;
        }
        next.epoch == self.applied.epoch && next.position == self.applied.position.saturating_add(1)
            || (self.applied.epoch == 0 && self.applied.position == 0)
    }

    pub fn apply(&mut self, pos: SourceLogPosition) {
        self.applied = pos;
    }
}
