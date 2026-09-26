//! Topology ladder + quorum helpers (004 MVP) + durable-ack wait (013 T062).

use spacestorage_types::{AckKind, StorageModeChoice, WriteAck};

#[derive(Debug, Clone)]
pub struct Topology {
    pub ladder: Vec<String>,
    pub quorum_domain: String,
}

impl Topology {
    pub fn default_lab() -> Self {
        Self {
            ladder: vec!["az".into()],
            quorum_domain: "lab".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quorum {
    One,
    Two,
}

impl Quorum {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "ONE" | "1" => Some(Self::One),
            "TWO" | "2" => Some(Self::Two),
            _ => None,
        }
    }

    pub fn required_acks(self) -> u32 {
        match self {
            Self::One => 1,
            Self::Two => 2,
        }
    }
}

/// TWO is never reinterpreted as min(2, live_replicas).
pub fn write_satisfied(required: Quorum, durable_acks_in_domain: u32, _live_replicas: u32) -> bool {
    durable_acks_in_domain >= required.required_acks()
}

/// True when the replica’s pinned drive has advanced past the record LSN.
pub fn durable_lsn_covers(drive_durable_lsn: u64, record_lsn: u64) -> bool {
    drive_durable_lsn >= record_lsn
}

/// Count only crash-durable acks (persistent/hybrid + durable kind).
pub fn count_durable_acks(acks: &[WriteAck]) -> u32 {
    acks.iter()
        .filter(|a| a.counts_as_crash_durable())
        .count() as u32
}

/// Quorum wait path: require counted durable acks whose LSN is covered by the
/// replica drive’s `durable_lsn`. Memory-kind / memory-mode acks never count.
///
/// `drive_durable_lsn` maps `(replica, drive_id) → durable_lsn` for pinned drives.
pub fn write_quorum_satisfied(
    required: Quorum,
    acks: &[WriteAck],
    drive_durable_lsn: &dyn Fn(&str, &str) -> u64,
    live_replicas: u32,
) -> bool {
    let counted = acks
        .iter()
        .filter(|a| {
            if !a.counts_as_crash_durable() {
                return false;
            }
            let dlsn = drive_durable_lsn(&a.replica, &a.drive_id);
            durable_lsn_covers(dlsn, a.lsn)
        })
        .count() as u32;
    write_satisfied(required, counted, live_replicas)
}

/// Label a local WAL ack for placement/reporting.
pub fn label_wal_ack(
    replica: impl Into<String>,
    drive_id: impl Into<String>,
    lsn: u64,
    wal_kind: AckKind,
    mode: StorageModeChoice,
    durable_lsn: u64,
) -> WriteAck {
    WriteAck::from_wal(replica, drive_id, lsn, wal_kind, mode, durable_lsn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_not_min_live() {
        assert!(!write_satisfied(Quorum::Two, 1, 1));
        assert!(write_satisfied(Quorum::Two, 2, 2));
        assert!(!write_satisfied(Quorum::Two, 1, 3));
    }

    #[test]
    fn memory_acks_do_not_satisfy_persistent_quorum() {
        let acks = vec![
            WriteAck {
                replica: "n1".into(),
                drive_id: "d1".into(),
                lsn: 10,
                kind: AckKind::Memory,
                mode: StorageModeChoice::Persistent,
            },
            WriteAck {
                replica: "n2".into(),
                drive_id: "d1".into(),
                lsn: 10,
                kind: AckKind::Durable,
                mode: StorageModeChoice::Memory,
            },
        ];
        let lookup = |_r: &str, _d: &str| 100u64;
        assert!(!write_quorum_satisfied(Quorum::One, &acks, &lookup, 2));
        assert_eq!(count_durable_acks(&acks), 0);
    }

    #[test]
    fn durable_acks_wait_for_drive_lsn() {
        let acks = vec![WriteAck {
            replica: "n1".into(),
            drive_id: "d1".into(),
            lsn: 10,
            kind: AckKind::Durable,
            mode: StorageModeChoice::Persistent,
        }];
        let lagging = |_r: &str, _d: &str| 5u64;
        let caught_up = |_r: &str, _d: &str| 10u64;
        assert!(!write_quorum_satisfied(Quorum::One, &acks, &lagging, 1));
        assert!(write_quorum_satisfied(Quorum::One, &acks, &caught_up, 1));
    }

    #[test]
    fn hybrid_durable_counts() {
        let ack = label_wal_ack(
            "n1",
            "d1",
            3,
            AckKind::Durable,
            StorageModeChoice::Hybrid,
            3,
        );
        assert!(ack.counts_as_crash_durable());
        assert_eq!(count_durable_acks(&[ack]), 1);
    }
}
