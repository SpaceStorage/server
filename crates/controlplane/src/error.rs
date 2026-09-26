//! Control-plane validation / refusal codes (006 data-model §Validation).

use crate::group::GroupId;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ControlPlaneError {
    #[error(
        "NotLeader{{group={group}, leader={leader:?}}}: write must go to primary (or wait for election)"
    )]
    NotLeader {
        group: GroupId,
        leader: Option<Uuid>,
    },

    #[error(
        "Minority{{group={group}}}: voter majority unreachable; retry when majority of voters recovers"
    )]
    Minority { group: GroupId },

    #[error("NotMember: process is not in cluster membership; cannot vote or take new replicas")]
    NotMember,

    #[error(
        "VoterSetOdd{{size={size}}}: voter set must be odd (1,3,5,…); propose an odd-sized set"
    )]
    VoterSetOdd { size: usize },

    #[error(
        "VoterSetMajorityLost: proposed membership change would lose majority at an accepted step; previous set remains"
    )]
    VoterSetMajorityLost,

    #[error(
        "StaleEpoch{{have={have}, need={need}}}: ordered append refused; re-grant lease and retry with current epoch"
    )]
    StaleEpoch { have: u64, need: u64 },

    #[error(
        "LeaseNotGranted{{container={container}}}: ordered write requires a leadership lease epoch"
    )]
    LeaseNotGranted { container: Uuid },

    #[error(
        "LeaseForbidden{{type={type_name}}}: leaderless datatype must not take a leadership lease"
    )]
    LeaseForbidden { type_name: String },

    #[error(
        "ExclusiveDataBlocked{{node={node}, containers={containers:?}}}: drain tenant replicas off controller voters before exclusive-data on"
    )]
    ExclusiveDataBlocked {
        node: Uuid,
        containers: Vec<Uuid>,
    },

    #[error(
        "Slice7Required{{op={op}}}: enable controlplane-ops (slice 7) for voter migrate / exclusive-data on"
    )]
    Slice7Required { op: String },

    #[error(
        "UnknownRaftFormat{{version={version}}}: isolate group dir and refuse start (015)"
    )]
    UnknownRaftFormat { version: u32 },

    #[error("io: {0}")]
    Io(String),

    #[error("{0}")]
    Msg(String),
}

pub type Result<T> = std::result::Result<T, ControlPlaneError>;
