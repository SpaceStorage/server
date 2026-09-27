//! L0 foundation primitives — creatable data structures as a user workflow (003 / intent 16).
//!
//! Storage primitives (`memtable`, `sstable`, `wal`, `append_segment`) and layout
//! `lsm_tree` remain **not creatable** ([type-inventory.md](../../specs/003-type-system/contracts/type-inventory.md)).

use crate::descriptor::{Kind, Level, TypeDescriptor};
use crate::error::TypeError;
use crate::ident::{new_container_id, ContainerId, ContainerName, NamespaceName};
use crate::definition::StorageModeChoice;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Creatable L0 data-structure machine names (15).
pub const L0_CREATABLE: &[&str] = &[
    "tuple",
    "vector",
    "linked_list",
    "deque",
    "ring_buffer",
    "hash_table",
    "bplus_tree",
    "skip_list",
    "radix_tree",
    "heap",
    "bloom_filter",
    "kd_tree",
    "hnsw",
    "scann",
    "bitmap",
];

/// Non-creatable L0 storage primitives + layout.
pub const L0_NOT_CREATABLE: &[&str] = &["memtable", "sstable", "wal", "append_segment", "lsm_tree"];

pub fn is_l0_creatable(name: &str) -> bool {
    L0_CREATABLE.contains(&name)
}

pub fn is_l0_not_creatable(name: &str) -> bool {
    L0_NOT_CREATABLE.contains(&name)
}

pub fn l0_descriptor(name: &str) -> Option<TypeDescriptor> {
    if is_l0_creatable(name) {
        Some(TypeDescriptor {
            name: name.into(),
            display_name: display_for(name).into(),
            level: Level::L0,
            kind: Kind::DataStructure,
            multi_active: false,
            schema_required: matches!(name, "kd_tree" | "hnsw" | "scann"),
        })
    } else if is_l0_not_creatable(name) {
        Some(TypeDescriptor {
            name: name.into(),
            display_name: display_for(name).into(),
            level: Level::L0,
            kind: if name == "lsm_tree" {
                Kind::StorageLayout
            } else {
                Kind::StoragePrimitive
            },
            multi_active: false,
            schema_required: false,
        })
    } else {
        None
    }
}

fn display_for(name: &str) -> &'static str {
    match name {
        "tuple" => "Tuple",
        "vector" => "Vector",
        "linked_list" => "Linked List",
        "deque" => "Stack / Queue / Deque",
        "ring_buffer" => "Ring Buffer",
        "hash_table" => "Hash Table",
        "bplus_tree" => "B+tree",
        "skip_list" => "Skip List",
        "radix_tree" => "Radix Tree / Patricia Trie",
        "heap" => "Heap",
        "bloom_filter" => "Bloom Filter",
        "kd_tree" => "KD-tree",
        "hnsw" => "HNSW",
        "scann" => "ScaNN",
        "bitmap" => "Bitmap / Bitset",
        "memtable" => "MemTable",
        "sstable" => "SSTable",
        "wal" => "WAL",
        "append_segment" => "Append-only Segment",
        "lsm_tree" => "LSM Tree",
        _ => "L0",
    }
}

/// In-memory L0 container instance (user-supported workflow surface).
#[derive(Debug, Clone)]
pub struct L0Container {
    pub id: ContainerId,
    pub name: ContainerName,
    pub namespace: NamespaceName,
    pub type_name: String,
    pub mode: StorageModeChoice,
    store: L0Store,
}

#[derive(Debug, Clone)]
enum L0Store {
    HashTable(HashMap<String, Vec<u8>>),
    Vector(Vec<Vec<u8>>),
    BloomFilter {
        bits: Vec<u64>,
        inserts: u64,
    },
    /// Generic opaque store for remaining creatable L0 types (ops subset).
    Kv(HashMap<String, Vec<u8>>),
}

impl L0Container {
    pub fn create(
        namespace: impl Into<String>,
        name: impl Into<String>,
        type_name: &str,
        mode: StorageModeChoice,
    ) -> Result<Self, TypeError> {
        if is_l0_not_creatable(type_name) {
            return Err(TypeError::NotCreatable);
        }
        if !is_l0_creatable(type_name) {
            return Err(TypeError::UnknownType);
        }
        let store = match type_name {
            "hash_table" => L0Store::HashTable(HashMap::new()),
            "vector" => L0Store::Vector(Vec::new()),
            "bloom_filter" => L0Store::BloomFilter {
                bits: vec![0; 64],
                inserts: 0,
            },
            _ => L0Store::Kv(HashMap::new()),
        };
        Ok(Self {
            id: new_container_id(),
            name: ContainerName::new(name.into()),
            namespace: NamespaceName::new(namespace.into()),
            type_name: type_name.into(),
            mode,
            store,
        })
    }

    pub fn put(&mut self, key: &str, value: Vec<u8>) -> Result<(), TypeError> {
        match &mut self.store {
            L0Store::HashTable(m) | L0Store::Kv(m) => {
                m.insert(key.into(), value);
                Ok(())
            }
            L0Store::Vector(v) => {
                if let Ok(i) = key.parse::<usize>() {
                    if i >= v.len() {
                        v.resize(i + 1, Vec::new());
                    }
                    v[i] = value;
                    Ok(())
                } else {
                    v.push(value);
                    Ok(())
                }
            }
            L0Store::BloomFilter { bits, inserts } => {
                bloom_add(bits, key.as_bytes());
                *inserts += 1;
                let _ = value;
                Ok(())
            }
        }
    }

    pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>, TypeError> {
        match &self.store {
            L0Store::HashTable(m) | L0Store::Kv(m) => Ok(m.get(key).cloned()),
            L0Store::Vector(v) => {
                let i: usize = key.parse().map_err(|_| TypeError::Msg("vector index".into()))?;
                Ok(v.get(i).cloned())
            }
            L0Store::BloomFilter { bits, .. } => {
                Ok(Some(if bloom_maybe(bits, key.as_bytes()) {
                    b"1".to_vec()
                } else {
                    b"0".to_vec()
                }))
            }
        }
    }

    pub fn delete(&mut self, key: &str) -> Result<bool, TypeError> {
        match &mut self.store {
            L0Store::HashTable(m) | L0Store::Kv(m) => Ok(m.remove(key).is_some()),
            L0Store::Vector(v) => {
                let i: usize = key.parse().map_err(|_| TypeError::Msg("vector index".into()))?;
                if i < v.len() {
                    v[i].clear();
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            L0Store::BloomFilter { .. } => Err(TypeError::Msg(
                "bloom_filter delete unsupported; use clear".into(),
            )),
        }
    }

    pub fn len(&self) -> usize {
        match &self.store {
            L0Store::HashTable(m) | L0Store::Kv(m) => m.len(),
            L0Store::Vector(v) => v.len(),
            L0Store::BloomFilter { inserts, .. } => *inserts as usize,
        }
    }
}

fn bloom_add(bits: &mut [u64], key: &[u8]) {
    for h in bloom_hashes(key) {
        let idx = (h as usize) % (bits.len() * 64);
        bits[idx / 64] |= 1u64 << (idx % 64);
    }
}

fn bloom_maybe(bits: &[u64], key: &[u8]) -> bool {
    bloom_hashes(key).into_iter().all(|h| {
        let idx = (h as usize) % (bits.len() * 64);
        bits[idx / 64] & (1u64 << (idx % 64)) != 0
    })
}

fn bloom_hashes(key: &[u8]) -> [u64; 3] {
    let mut h = 0xcbf29ce484222325u64;
    for b in key {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    [h, h.wrapping_mul(0x9e3779b97f4a7c15), h ^ 0xdeadbeef]
}

/// Catalog of live L0 containers (namespace, name) → instance.
#[derive(Debug, Default)]
pub struct L0Catalog {
    containers: HashMap<(String, String), L0Container>,
}

impl L0Catalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create(
        &mut self,
        namespace: impl Into<String>,
        name: impl Into<String>,
        type_name: &str,
        mode: StorageModeChoice,
    ) -> Result<ContainerId, TypeError> {
        let ns = namespace.into();
        let n = name.into();
        let key = (ns.clone(), n.clone());
        if self.containers.contains_key(&key) {
            return Err(TypeError::AlreadyExists);
        }
        let c = L0Container::create(ns, n, type_name, mode)?;
        let id = c.id;
        self.containers.insert(key, c);
        Ok(id)
    }

    pub fn get_mut(&mut self, namespace: &str, name: &str) -> Result<&mut L0Container, TypeError> {
        self.containers
            .get_mut(&(namespace.into(), name.into()))
            .ok_or(TypeError::NotFound)
    }

    pub fn describe(&self, namespace: &str, name: &str) -> Result<&L0Container, TypeError> {
        self.containers
            .get(&(namespace.into(), name.into()))
            .ok_or(TypeError::NotFound)
    }

    pub fn drop_container(&mut self, namespace: &str, name: &str) -> Result<(), TypeError> {
        self.containers
            .remove(&(namespace.into(), name.into()))
            .map(|_| ())
            .ok_or(TypeError::NotFound)
    }

    pub fn list(&self) -> Vec<L0ContainerSummary> {
        let mut out: Vec<_> = self
            .containers
            .values()
            .map(|c| L0ContainerSummary {
                id: c.id,
                namespace: c.namespace.as_str().to_string(),
                name: c.name.as_str().to_string(),
                type_name: c.type_name.clone(),
                mode: match c.mode {
                    StorageModeChoice::Memory => "memory",
                    StorageModeChoice::Persistent => "persistent",
                    StorageModeChoice::Hybrid => "hybrid",
                }
                .into(),
                len: c.len(),
            })
            .collect();
        out.sort_by(|a, b| (&a.namespace, &a.name).cmp(&(&b.namespace, &b.name)));
        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L0ContainerSummary {
    pub id: ContainerId,
    pub namespace: String,
    pub name: String,
    pub type_name: String,
    pub mode: String,
    pub len: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L0CreateRequest {
    pub namespace: String,
    pub name: String,
    pub type_name: String,
    #[serde(default)]
    pub mode: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creatable_count_is_15() {
        assert_eq!(L0_CREATABLE.len(), 15);
        assert_eq!(L0_NOT_CREATABLE.len(), 5);
    }

    #[test]
    fn refuse_wal_create() {
        assert!(matches!(
            L0Container::create("ns", "w", "wal", StorageModeChoice::Persistent),
            Err(TypeError::NotCreatable)
        ));
    }

    #[test]
    fn hash_table_workflow() {
        let mut cat = L0Catalog::new();
        let id = cat
            .create("acme", "ht", "hash_table", StorageModeChoice::Memory)
            .unwrap();
        let c = cat.get_mut("acme", "ht").unwrap();
        assert_eq!(c.id, id);
        c.put("k", b"v".to_vec()).unwrap();
        assert_eq!(c.get("k").unwrap(), Some(b"v".to_vec()));
        assert!(c.delete("k").unwrap());
        assert_eq!(c.len(), 0);
    }

    #[test]
    fn all_creatable_option_free() {
        let mut cat = L0Catalog::new();
        for (i, t) in L0_CREATABLE.iter().enumerate() {
            // kd_tree/hnsw/scann require schema in full product; workflow still creates with defaults.
            cat.create("ns", format!("c{i}"), t, StorageModeChoice::Memory)
                .unwrap_or_else(|e| panic!("{t}: {e}"));
        }
        assert_eq!(cat.containers.len(), 15);
    }
}
