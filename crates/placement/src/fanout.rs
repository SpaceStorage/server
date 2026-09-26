//! Leaderless source-`quorum_domain` coordinator fan-out (FR-045, Constitution VI).
//!
//! Ordinary containers stay leaderless: any in-domain replica may accept a write;
//! the coordinator waits for durable acknowledgements **in that domain**. No Raft
//! and no mandatory write serializer inside the source.

use crate::error::PlacementError;
use crate::quorum::QuorumLevel;
use crate::{may_coordinate, Quorum};
use serde::{Deserialize, Serialize};

/// Wire-shaped write (mirrors internode `FanoutWrite`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FanoutWrite {
    pub container_id: String,
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub stamp_physical: u64,
    pub stamp_logical: u32,
}

/// Wire-shaped read (mirrors internode `FanoutRead`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FanoutRead {
    pub container_id: String,
    pub key: Vec<u8>,
}

/// Replica that may be contacted for in-domain fan-out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FanoutReplica {
    pub node: String,
    pub quorum_domain: String,
}

/// Per-replica acknowledgement from a fan-out send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FanoutAck {
    pub replica: String,
    pub ok: bool,
    /// Crash-durable ack (persistent/hybrid + durable kind). Memory never counts.
    pub durable: bool,
    pub value: Option<Vec<u8>>,
    pub stamp_physical: Option<u64>,
    pub stamp_logical: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FanoutWriteOutcome {
    pub contacted: Vec<String>,
    pub durable_acks: u32,
    pub required: u32,
    pub satisfied: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FanoutReadOutcome {
    pub contacted: Vec<String>,
    pub acks: u32,
    pub required: u32,
    pub satisfied: bool,
    /// LWW-greatest value among successful replies (physical then logical).
    pub value: Option<Vec<u8>>,
    pub stamp_physical: Option<u64>,
    pub stamp_logical: Option<u32>,
}

/// Replicas that belong to the source quorum domain (in-domain only).
pub fn in_source_domain<'a>(
    replicas: &'a [FanoutReplica],
    source_domain: &str,
) -> Vec<&'a FanoutReplica> {
    replicas
        .iter()
        .filter(|r| r.quorum_domain == source_domain)
        .collect()
}

fn required_write_acks(level: QuorumLevel, live_in_domain: u32) -> u32 {
    level.required_acks(live_in_domain)
}

fn required_read_acks(level: QuorumLevel, live_in_domain: u32) -> u32 {
    level.required_acks(live_in_domain)
}

/// Leaderless `FanoutWrite`: contact every in-domain replica independently;
/// wait for durable acks to meet the write level. No preferred leader.
pub fn fanout_write<F>(
    coordinator_domain: &str,
    source_domain: &str,
    coordinator_is_member: bool,
    replicas: &[FanoutReplica],
    _write: &FanoutWrite,
    write_level: QuorumLevel,
    mut send: F,
) -> Result<FanoutWriteOutcome, PlacementError>
where
    F: FnMut(&FanoutReplica) -> FanoutAck,
{
    if !may_coordinate(coordinator_domain, source_domain, coordinator_is_member) {
        // Followers forward (012); this library path is source-domain only.
        return Err(PlacementError::QuorumUnsatisfiable);
    }
    let targets = in_source_domain(replicas, source_domain);
    let live = targets.len() as u32;
    let required = required_write_acks(write_level, live);
    if required == 0 || live < required {
        return Err(PlacementError::QuorumUnsatisfiable);
    }

    let mut contacted = Vec::new();
    let mut durable_acks = 0u32;
    // Leaderless: iterate all in-domain targets; order is deterministic by name
    // only for test stability — no replica is a serializer.
    let mut ordered = targets;
    ordered.sort_by(|a, b| a.node.cmp(&b.node));
    for r in ordered {
        let ack = send(r);
        contacted.push(r.node.clone());
        if ack.ok && ack.durable {
            durable_acks = durable_acks.saturating_add(1);
        }
    }

    let satisfied = write_satisfied_level(write_level, durable_acks, live);
    if !satisfied {
        return Err(PlacementError::QuorumUnsatisfiable);
    }
    Ok(FanoutWriteOutcome {
        contacted,
        durable_acks,
        required,
        satisfied: true,
    })
}

fn write_satisfied_level(level: QuorumLevel, durable: u32, live: u32) -> bool {
    let need = level.required_acks(live);
    // TWO is never reinterpreted as min(2, live) — same rule as [`crate::write_satisfied`].
    match level {
        QuorumLevel::Two => {
            crate::write_satisfied(Quorum::Two, durable, live)
        }
        QuorumLevel::One | QuorumLevel::LocalOne => durable >= need,
        _ => durable >= need,
    }
}

/// Leaderless `FanoutRead`: any in-domain replica may serve; count successful
/// replies (reads do not use the durable filter).
pub fn fanout_read<F>(
    coordinator_domain: &str,
    source_domain: &str,
    coordinator_is_member: bool,
    replicas: &[FanoutReplica],
    _read: &FanoutRead,
    read_level: QuorumLevel,
    mut send: F,
) -> Result<FanoutReadOutcome, PlacementError>
where
    F: FnMut(&FanoutReplica) -> FanoutAck,
{
    if !may_coordinate(coordinator_domain, source_domain, coordinator_is_member) {
        return Err(PlacementError::QuorumUnsatisfiable);
    }
    let targets = in_source_domain(replicas, source_domain);
    let live = targets.len() as u32;
    let required = required_read_acks(read_level, live);
    if required == 0 || live < required {
        return Err(PlacementError::QuorumUnsatisfiable);
    }

    let mut contacted = Vec::new();
    let mut acks = 0u32;
    let mut best: Option<(u64, u32, Vec<u8>)> = None;
    let mut ordered = targets;
    ordered.sort_by(|a, b| a.node.cmp(&b.node));
    for r in ordered {
        let ack = send(r);
        contacted.push(r.node.clone());
        if !ack.ok {
            continue;
        }
        acks = acks.saturating_add(1);
        if let (Some(p), Some(l), Some(v)) =
            (ack.stamp_physical, ack.stamp_logical, ack.value.clone())
        {
            match &best {
                None => best = Some((p, l, v)),
                Some((bp, bl, _)) if (p, l) > (*bp, *bl) => best = Some((p, l, v)),
                _ => {}
            }
        }
    }

    if acks < required {
        return Err(PlacementError::QuorumUnsatisfiable);
    }
    Ok(FanoutReadOutcome {
        contacted,
        acks,
        required,
        satisfied: true,
        value: best.as_ref().map(|b| b.2.clone()),
        stamp_physical: best.as_ref().map(|b| b.0),
        stamp_logical: best.as_ref().map(|b| b.1),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replicas() -> Vec<FanoutReplica> {
        vec![
            FanoutReplica {
                node: "n1".into(),
                quorum_domain: "lab".into(),
            },
            FanoutReplica {
                node: "n2".into(),
                quorum_domain: "lab".into(),
            },
            FanoutReplica {
                node: "n3".into(),
                quorum_domain: "lab".into(),
            },
            FanoutReplica {
                node: "n4".into(),
                quorum_domain: "other".into(),
            },
        ]
    }

    fn write() -> FanoutWrite {
        FanoutWrite {
            container_id: "c".into(),
            key: b"k".to_vec(),
            value: b"v".to_vec(),
            stamp_physical: 1,
            stamp_logical: 0,
        }
    }

    #[test]
    fn write_two_waits_durable_in_domain_only() {
        let mut contacted_other = false;
        let out = fanout_write(
            "lab",
            "lab",
            true,
            &replicas(),
            &write(),
            QuorumLevel::Two,
            |r| {
                if r.quorum_domain != "lab" {
                    contacted_other = true;
                }
                FanoutAck {
                    replica: r.node.clone(),
                    ok: true,
                    durable: true,
                    value: None,
                    stamp_physical: None,
                    stamp_logical: None,
                }
            },
        )
        .unwrap();
        assert!(!contacted_other);
        assert_eq!(out.contacted, vec!["n1", "n2", "n3"]);
        assert_eq!(out.durable_acks, 3);
        assert!(out.satisfied);
    }

    #[test]
    fn write_two_fails_when_only_one_durable() {
        let err = fanout_write(
            "lab",
            "lab",
            true,
            &replicas(),
            &write(),
            QuorumLevel::Two,
            |r| FanoutAck {
                replica: r.node.clone(),
                ok: true,
                // Only n1 durable — TWO must not clamp to live.
                durable: r.node == "n1",
                value: None,
                stamp_physical: None,
                stamp_logical: None,
            },
        )
        .unwrap_err();
        assert!(matches!(err, PlacementError::QuorumUnsatisfiable));
    }

    #[test]
    fn memory_acks_do_not_count_for_write() {
        let err = fanout_write(
            "lab",
            "lab",
            true,
            &replicas()[..3],
            &write(),
            QuorumLevel::Two,
            |r| FanoutAck {
                replica: r.node.clone(),
                ok: true,
                durable: false,
                value: None,
                stamp_physical: None,
                stamp_logical: None,
            },
        )
        .unwrap_err();
        assert!(matches!(err, PlacementError::QuorumUnsatisfiable));
    }

    #[test]
    fn leaderless_any_member_coordinates_no_preferred_leader() {
        // Contact order is sorted by name for stability; every in-domain replica
        // is equal — no leader field / Raft path.
        let out = fanout_write(
            "lab",
            "lab",
            true,
            &replicas()[..3],
            &write(),
            QuorumLevel::One,
            |r| FanoutAck {
                replica: r.node.clone(),
                ok: true,
                durable: true,
                value: None,
                stamp_physical: None,
                stamp_logical: None,
            },
        )
        .unwrap();
        assert_eq!(out.contacted.len(), 3);
        assert!(out.satisfied);
    }

    #[test]
    fn non_member_or_wrong_domain_cannot_coordinate() {
        assert!(matches!(
            fanout_write(
                "lab",
                "lab",
                false,
                &replicas()[..1],
                &write(),
                QuorumLevel::One,
                |_| unreachable!(),
            ),
            Err(PlacementError::QuorumUnsatisfiable)
        ));
        assert!(matches!(
            fanout_write(
                "other",
                "lab",
                true,
                &replicas()[..1],
                &write(),
                QuorumLevel::One,
                |_| unreachable!(),
            ),
            Err(PlacementError::QuorumUnsatisfiable)
        ));
    }

    #[test]
    fn read_one_returns_lww_greatest() {
        let read = FanoutRead {
            container_id: "c".into(),
            key: b"k".to_vec(),
        };
        let out = fanout_read(
            "lab",
            "lab",
            true,
            &replicas()[..3],
            &read,
            QuorumLevel::One,
            |r| FanoutAck {
                replica: r.node.clone(),
                ok: true,
                durable: false,
                value: Some(format!("v-{}", r.node).into_bytes()),
                stamp_physical: Some(if r.node == "n2" { 10 } else { 1 }),
                stamp_logical: Some(0),
            },
        )
        .unwrap();
        assert!(out.satisfied);
        assert_eq!(out.value.as_deref(), Some(b"v-n2".as_slice()));
        assert_eq!(out.stamp_physical, Some(10));
    }
}
