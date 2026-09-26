//! Per-namespace Raft apply machine (schemas, containers, leases).

use crate::lease::LeadershipLease;
use crate::membership::VoterSet;
use serde::{Deserialize, Serialize};
use spacestorage_clocks::HlcStamp;
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NamespaceState {
    pub namespace_id: Uuid,
    pub schemas: BTreeMap<String, serde_json::Value>,
    pub containers: BTreeMap<Uuid, serde_json::Value>,
    pub leases: BTreeMap<Uuid, LeadershipLease>,
    pub voter_set: Option<VoterSet>,
    pub hlc: Option<HlcStamp>,
}

impl NamespaceState {
    pub fn new(namespace_id: Uuid, voter_set: VoterSet) -> Self {
        Self {
            namespace_id,
            schemas: BTreeMap::new(),
            containers: BTreeMap::new(),
            leases: BTreeMap::new(),
            voter_set: Some(voter_set),
            hlc: None,
        }
    }

    pub fn apply_definition(&mut self, container_id: Uuid, def: serde_json::Value, hlc: HlcStamp) {
        self.containers.insert(container_id, def);
        self.hlc = Some(hlc);
    }

    pub fn apply_schema(&mut self, name: String, schema: serde_json::Value, hlc: HlcStamp) {
        self.schemas.insert(name, schema);
        self.hlc = Some(hlc);
    }

    pub fn put_lease(&mut self, lease: LeadershipLease, hlc: HlcStamp) {
        self.leases.insert(lease.container_id, lease);
        self.hlc = Some(hlc);
    }
}
