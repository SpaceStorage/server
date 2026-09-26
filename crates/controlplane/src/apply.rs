//! Apply committed Raft entries into cluster / namespace state machines.

use crate::cluster::{ClusterOp, ClusterState};
use crate::error::{ControlPlaneError, Result};
use crate::group::GroupId;
use crate::membership::MembershipOp;
use crate::namespace::NamespaceState;
use crate::raft_store::{RaftBody, RaftLogRecord};
use spacestorage_clocks::HlcStamp;
use std::collections::BTreeMap;
use uuid::Uuid;

pub struct ApplyEngine {
    pub cluster: ClusterState,
    pub namespaces: BTreeMap<Uuid, NamespaceState>,
}

impl ApplyEngine {
    pub fn new(cluster: ClusterState) -> Self {
        Self {
            cluster,
            namespaces: BTreeMap::new(),
        }
    }

    pub fn apply_record(&mut self, rec: &RaftLogRecord) -> Result<()> {
        match &rec.body {
            RaftBody::Noop => Ok(()),
            RaftBody::Cluster(v) => {
                let op: ClusterOp = serde_json::from_value(v.clone())
                    .map_err(|e| ControlPlaneError::Msg(e.to_string()))?;
                // Creating a namespace starts an empty NamespaceState with cluster voters.
                if let ClusterOp::CreateNamespace { id, .. } = &op {
                    let vs = self.cluster.voter_set.clone();
                    let mut ns_vs = vs.clone();
                    ns_vs.group = GroupId::Namespace(*id);
                    self.namespaces
                        .insert(*id, NamespaceState::new(*id, ns_vs));
                }
                if let ClusterOp::DeleteNamespace { id } = &op {
                    self.namespaces.remove(id);
                }
                self.cluster.apply(op, rec.hlc.clone());
                Ok(())
            }
            RaftBody::Namespace(v) => {
                let ns_id = v
                    .get("namespace_id")
                    .and_then(|x| x.as_str())
                    .and_then(|s| Uuid::parse_str(s).ok())
                    .ok_or_else(|| ControlPlaneError::Msg("namespace body missing id".into()))?;
                let ns = self.namespaces.get_mut(&ns_id).ok_or_else(|| {
                    ControlPlaneError::Msg(format!("unknown namespace {ns_id}"))
                })?;
                if let Some(cid) = v.get("container_id").and_then(|x| x.as_str()).and_then(|s| Uuid::parse_str(s).ok()) {
                    if let Some(def) = v.get("definition") {
                        ns.apply_definition(cid, def.clone(), rec.hlc.clone());
                    }
                }
                if let (Some(name), Some(schema)) = (
                    v.get("schema_name").and_then(|x| x.as_str()).map(str::to_string),
                    v.get("schema"),
                ) {
                    ns.apply_schema(name, schema.clone(), rec.hlc.clone());
                }
                Ok(())
            }
            RaftBody::Membership(op) => self.apply_membership(op.clone(), rec.hlc.clone()),
        }
    }

    fn apply_membership(&mut self, op: MembershipOp, hlc: HlcStamp) -> Result<()> {
        let members: std::collections::BTreeSet<_> =
            self.cluster.members.keys().copied().collect();
        let next = op.apply(&self.cluster.voter_set, &members)?;
        self.cluster.voter_set = next;
        self.cluster.hlc = hlc;
        Ok(())
    }
}
