//! Named CDC product beyond Log Stream + WAL (intent 16 / 003 / 013).
//!
//! Log Stream remains the L3 append type; WAL remains durability. This module is the
//! **change-data-capture** product surface: named streams of row-level change events
//! with consumer offsets, independent of Kafka ingest (09).

use crate::error::TypeError;
use crate::ident::{new_container_id, ContainerId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CdcOp {
    Insert,
    Update,
    Delete,
    Tombstone,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdcEvent {
    pub seq: u64,
    pub namespace: String,
    pub container: String,
    pub key: String,
    pub op: CdcOp,
    #[serde(default)]
    pub before: Option<Vec<u8>>,
    #[serde(default)]
    pub after: Option<Vec<u8>>,
    pub hlc_physical: u64,
    pub hlc_logical: u32,
}

#[derive(Debug)]
pub struct CdcStream {
    pub id: ContainerId,
    pub name: String,
    pub namespace: String,
    /// Source container this CDC stream watches.
    pub source_container: String,
    events: Vec<CdcEvent>,
    next_seq: AtomicU64,
    /// Consumer group → next seq to read.
    offsets: HashMap<String, u64>,
}

impl CdcStream {
    pub fn new(
        namespace: impl Into<String>,
        name: impl Into<String>,
        source_container: impl Into<String>,
    ) -> Self {
        Self {
            id: new_container_id(),
            name: name.into(),
            namespace: namespace.into(),
            source_container: source_container.into(),
            events: Vec::new(),
            next_seq: AtomicU64::new(1),
            offsets: HashMap::new(),
        }
    }

    pub fn publish(&mut self, mut event: CdcEvent) -> u64 {
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        event.seq = seq;
        self.events.push(event);
        seq
    }

    pub fn read_from(&self, from_seq: u64, limit: usize) -> Vec<&CdcEvent> {
        self.events
            .iter()
            .filter(|e| e.seq >= from_seq)
            .take(limit)
            .collect()
    }

    pub fn commit_offset(&mut self, consumer: impl Into<String>, next_seq: u64) {
        self.offsets.insert(consumer.into(), next_seq);
    }

    pub fn consumer_offset(&self, consumer: &str) -> u64 {
        self.offsets.get(consumer).copied().unwrap_or(1)
    }
}

#[derive(Debug, Default)]
pub struct CdcCatalog {
    streams: HashMap<(String, String), CdcStream>,
}

impl CdcCatalog {
    pub fn create(
        &mut self,
        namespace: impl Into<String>,
        name: impl Into<String>,
        source_container: impl Into<String>,
    ) -> Result<ContainerId, TypeError> {
        let ns = namespace.into();
        let n = name.into();
        let key = (ns.clone(), n.clone());
        if self.streams.contains_key(&key) {
            return Err(TypeError::AlreadyExists);
        }
        let s = CdcStream::new(ns, n, source_container);
        let id = s.id;
        self.streams.insert(key, s);
        Ok(id)
    }

    pub fn get_mut(&mut self, namespace: &str, name: &str) -> Result<&mut CdcStream, TypeError> {
        self.streams
            .get_mut(&(namespace.into(), name.into()))
            .ok_or(TypeError::NotFound)
    }

    pub fn get(&self, namespace: &str, name: &str) -> Result<&CdcStream, TypeError> {
        self.streams
            .get(&(namespace.into(), name.into()))
            .ok_or(TypeError::NotFound)
    }

    pub fn list(&self) -> Vec<CdcStreamSummary> {
        let mut out: Vec<_> = self
            .streams
            .values()
            .map(|s| CdcStreamSummary {
                id: s.id,
                namespace: s.namespace.clone(),
                name: s.name.clone(),
                source_container: s.source_container.clone(),
                event_count: s.events.len(),
            })
            .collect();
        out.sort_by(|a, b| (&a.namespace, &a.name).cmp(&(&b.namespace, &b.name)));
        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdcStreamSummary {
    pub id: ContainerId,
    pub namespace: String,
    pub name: String,
    pub source_container: String,
    pub event_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cdc_beyond_log_stream() {
        let mut cat = CdcCatalog::default();
        cat.create("acme", "orders_cdc", "orders").unwrap();
        let s = cat.get_mut("acme", "orders_cdc").unwrap();
        let seq = s.publish(CdcEvent {
            seq: 0,
            namespace: "acme".into(),
            container: "orders".into(),
            key: "o1".into(),
            op: CdcOp::Insert,
            before: None,
            after: Some(b"{}".to_vec()),
            hlc_physical: 1,
            hlc_logical: 0,
        });
        assert_eq!(seq, 1);
        s.commit_offset("billing", 2);
        assert_eq!(s.consumer_offset("billing"), 2);
        assert_eq!(s.read_from(1, 10).len(), 1);
    }
}
