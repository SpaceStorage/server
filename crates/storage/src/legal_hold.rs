//! GDPR erase + legal-hold product workflows (intent 16 / 010 / 013 tombstone).
//!
//! Residency remains labels + placement (`004`). Erase is delete/tombstone with
//! retention gates; legal hold blocks erase until released.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum LegalError {
    #[error("legal_hold_active: {0}")]
    HoldActive(String),
    #[error("not_found")]
    NotFound,
    #[error("already_erased")]
    AlreadyErased,
    #[error("{0}")]
    Msg(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LegalHold {
    pub id: Uuid,
    pub namespace: String,
    pub container: String,
    /// Optional row key; None = container-wide hold.
    pub key: Option<String>,
    pub reason: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EraseRequest {
    pub namespace: String,
    pub container: String,
    pub key: String,
    pub requested_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EraseRecord {
    pub request: EraseRequest,
    pub tombstone_seq: u64,
    pub completed_at_ms: u64,
}

#[derive(Debug, Default)]
pub struct LegalHoldStore {
    holds: HashMap<Uuid, LegalHold>,
    /// (namespace, container, key) currently erased (tombstoned).
    erased: HashSet<(String, String, String)>,
    next_tombstone: u64,
    records: Vec<EraseRecord>,
}

impl LegalHoldStore {
    pub fn place_hold(&mut self, hold: LegalHold) -> Uuid {
        let id = hold.id;
        self.holds.insert(id, hold);
        id
    }

    pub fn release_hold(&mut self, id: Uuid) -> Result<(), LegalError> {
        self.holds
            .remove(&id)
            .map(|_| ())
            .ok_or(LegalError::NotFound)
    }

    pub fn active_holds(&self, namespace: &str, container: &str, key: Option<&str>) -> Vec<&LegalHold> {
        self.holds
            .values()
            .filter(|h| {
                h.namespace == namespace
                    && h.container == container
                    && match (&h.key, key) {
                        (None, _) => true,
                        (Some(hk), Some(k)) => hk == k,
                        (Some(_), None) => true,
                    }
            })
            .collect()
    }

    /// GDPR erase: refuse while any legal hold covers the key; otherwise tombstone.
    pub fn erase(&mut self, req: EraseRequest) -> Result<EraseRecord, LegalError> {
        let key = (
            req.namespace.clone(),
            req.container.clone(),
            req.key.clone(),
        );
        if self.erased.contains(&key) {
            return Err(LegalError::AlreadyErased);
        }
        let blocking: Vec<_> = self
            .active_holds(&req.namespace, &req.container, Some(&req.key))
            .into_iter()
            .map(|h| h.id.to_string())
            .collect();
        if !blocking.is_empty() {
            return Err(LegalError::HoldActive(blocking.join(",")));
        }
        self.next_tombstone += 1;
        let rec = EraseRecord {
            tombstone_seq: self.next_tombstone,
            completed_at_ms: req.requested_at_ms,
            request: req,
        };
        self.erased.insert(key);
        self.records.push(rec.clone());
        Ok(rec)
    }

    pub fn is_erased(&self, namespace: &str, container: &str, key: &str) -> bool {
        self.erased
            .contains(&(namespace.into(), container.into(), key.into()))
    }

    pub fn records(&self) -> &[EraseRecord] {
        &self.records
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hold_blocks_erase_then_allows() {
        let mut store = LegalHoldStore::default();
        let hid = Uuid::now_v7();
        store.place_hold(LegalHold {
            id: hid,
            namespace: "acme".into(),
            container: "users".into(),
            key: Some("u1".into()),
            reason: "litigation".into(),
            created_at_ms: 1,
        });
        let req = EraseRequest {
            namespace: "acme".into(),
            container: "users".into(),
            key: "u1".into(),
            requested_at_ms: 2,
        };
        assert!(matches!(
            store.erase(req.clone()),
            Err(LegalError::HoldActive(_))
        ));
        store.release_hold(hid).unwrap();
        let rec = store.erase(req).unwrap();
        assert_eq!(rec.tombstone_seq, 1);
        assert!(store.is_erased("acme", "users", "u1"));
    }
}
