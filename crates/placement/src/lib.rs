//! Topology ladder + placement/quorum (004 first-binary).

pub mod error;
pub mod fanout;
pub mod planner;
pub mod quorum;
pub mod replica;
pub mod stamp;
pub mod topology;

pub use error::PlacementError;
pub use fanout::{
    fanout_read, fanout_write, in_source_domain, FanoutAck, FanoutRead, FanoutReadOutcome,
    FanoutReplica, FanoutWrite, FanoutWriteOutcome,
};
pub use planner::{
    finest_ladder_key, plan_replicas, resolve_anti_affinity_keys, PlanRequest, PlacementPlan,
};
pub use quorum::{product_defaults, QuorumLevel};
pub use stamp::VersionStamp;
pub use topology::{TopologyLadder, TopologyView};

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

/// Count crash-durable acks only from replicas in `source_domain` (FR-006 / T072).
pub fn count_durable_acks(acks: &[WriteAck], source_domain: &str) -> u32 {
    acks.iter()
        .filter(|a| a.quorum_domain == source_domain && a.counts_as_crash_durable())
        .count() as u32
}

/// Quorum wait path: require counted durable acks in `source_domain` whose LSN is
/// covered by the replica drive’s `durable_lsn`. Memory-kind / memory-mode acks never count.
pub fn write_quorum_satisfied(
    required: Quorum,
    acks: &[WriteAck],
    source_domain: &str,
    drive_durable_lsn: &dyn Fn(&str, &str) -> u64,
    live_replicas: u32,
) -> bool {
    let counted = acks
        .iter()
        .filter(|a| {
            if a.quorum_domain != source_domain || !a.counts_as_crash_durable() {
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
    quorum_domain: impl Into<String>,
) -> WriteAck {
    WriteAck::from_wal(
        replica,
        drive_id,
        lsn,
        wal_kind,
        mode,
        durable_lsn,
        quorum_domain,
    )
}

/// Leaderless coordinator: any admitted member in the source quorum_domain may coordinate.
pub fn may_coordinate(member_domain: &str, source_domain: &str, is_member: bool) -> bool {
    is_member && member_domain == source_domain
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
                quorum_domain: "lab".into(),
            },
            WriteAck {
                replica: "n2".into(),
                drive_id: "d1".into(),
                lsn: 10,
                kind: AckKind::Durable,
                mode: StorageModeChoice::Memory,
                quorum_domain: "lab".into(),
            },
        ];
        let lookup = |_r: &str, _d: &str| 100u64;
        assert!(!write_quorum_satisfied(Quorum::One, &acks, "lab", &lookup, 2));
        assert_eq!(count_durable_acks(&acks, "lab"), 0);
    }

    #[test]
    fn durable_acks_wait_for_drive_lsn() {
        let acks = vec![WriteAck {
            replica: "n1".into(),
            drive_id: "d1".into(),
            lsn: 10,
            kind: AckKind::Durable,
            mode: StorageModeChoice::Persistent,
            quorum_domain: "lab".into(),
        }];
        let lagging = |_r: &str, _d: &str| 5u64;
        let caught_up = |_r: &str, _d: &str| 10u64;
        assert!(!write_quorum_satisfied(Quorum::One, &acks, "lab", &lagging, 1));
        assert!(write_quorum_satisfied(Quorum::One, &acks, "lab", &caught_up, 1));
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
            "lab",
        );
        assert!(ack.counts_as_crash_durable());
        assert_eq!(count_durable_acks(&[ack], "lab"), 1);
    }

    #[test]
    fn follower_domain_acks_do_not_count() {
        let acks = vec![
            WriteAck {
                replica: "follower".into(),
                drive_id: "d1".into(),
                lsn: 10,
                kind: AckKind::Durable,
                mode: StorageModeChoice::Persistent,
                quorum_domain: "other".into(),
            },
            WriteAck {
                replica: "source".into(),
                drive_id: "d1".into(),
                lsn: 10,
                kind: AckKind::Durable,
                mode: StorageModeChoice::Persistent,
                quorum_domain: "lab".into(),
            },
        ];
        assert_eq!(count_durable_acks(&acks, "lab"), 1);
        let lookup = |_r: &str, _d: &str| 100u64;
        assert!(write_quorum_satisfied(Quorum::One, &acks, "lab", &lookup, 2));
        assert!(!write_quorum_satisfied(Quorum::Two, &acks, "lab", &lookup, 2));
    }

    #[test]
    fn leaderless_same_domain() {
        assert!(may_coordinate("lab", "lab", true));
        assert!(!may_coordinate("lab", "other", true));
        assert!(!may_coordinate("lab", "lab", false));
    }
}
