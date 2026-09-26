//! Checkpoint of covered_lsn (prefix truncate gated by retain rules — stub).

use crate::Checkpoint;

pub fn stub_checkpoint(drive_id: impl Into<String>, covered_lsn: u64) -> Checkpoint {
    Checkpoint {
        drive_id: drive_id.into(),
        covered_lsn,
        at_hlc: 0,
    }
}
