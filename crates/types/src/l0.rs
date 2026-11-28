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

/// Creatable L0 data-structure machine names (16).
///
/// `hyperloglog` is the `017` Redis-dialect cardinality-sketch extension
/// (registered through the `003` FR-045–FR-049 path; it changes no baseline
/// descriptor).
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
    "hyperloglog",
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
        // `017` extension path: `hyperloglog` declares its operation set and
        // documented standard error here, changing no baseline descriptor.
        let (operations, standard_error): (Vec<&'static str>, Option<&'static str>) =
            if name == "hyperloglog" {
                (
                    vec!["add", "count", "merge", "clear"],
                    Some("1.04/sqrt(2^precision)"),
                )
            } else {
                (Vec::new(), None)
            };
        Some(TypeDescriptor {
            name: name.into(),
            display_name: display_for(name).into(),
            level: Level::L0,
            kind: Kind::DataStructure,
            multi_active: false,
            schema_required: matches!(name, "kd_tree" | "hnsw" | "scann"),
            snapshot_capable: false,
            introduced_in: crate::descriptor::ProductVersion::FIRST_BINARY,
            operations,
            standard_error,
            canonical_version: 1,
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
            snapshot_capable: false,
            introduced_in: crate::descriptor::ProductVersion::FIRST_BINARY,
            operations: Vec::new(),
            standard_error: None,
            canonical_version: 1,
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
        "hyperloglog" => "HyperLogLog",
        "memtable" => "MemTable",
        "sstable" => "SSTable",
        "wal" => "WAL",
        "append_segment" => "Append-only Segment",
        "lsm_tree" => "LSM Tree",
        _ => "L0",
    }
}

/// Default HyperLogLog precision — `2^14` registers, matching Redis (`017` FR-017).
pub const HLL_DEFAULT_PRECISION: u8 = 14;
/// Lower bound of a usable precision (`m = 16` registers).
pub const HLL_MIN_PRECISION: u8 = 4;
/// Upper bound of a usable precision (`m = 65536` registers).
pub const HLL_MAX_PRECISION: u8 = 16;

/// HyperLogLog cardinality sketch — the registered L0 `hyperloglog` primitive
/// (`017` US5, data-model §8). The algorithm lives here as the shared abstract
/// interface; the Redis handler only encodes/decodes the entry payload built
/// from this type (no protocol-private encoding).
///
/// Hashing is a fixed MurmurHash64A (never `DefaultHasher`), so estimates are
/// run-stable and the documented `1.04/sqrt(m)` accuracy bound is directly
/// testable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HyperLogLog {
    /// Frozen at create; register count is `1 << precision`.
    precision: u8,
    /// `1 << precision` registers holding the max observed rank + 1 (`0` = empty).
    registers: Vec<u8>,
}

impl Default for HyperLogLog {
    fn default() -> Self {
        Self::new(HLL_DEFAULT_PRECISION)
    }
}

impl HyperLogLog {
    /// New sketch at `precision` (clamped to `4..=16`, default 14).
    pub fn new(precision: u8) -> Self {
        let p = precision.clamp(HLL_MIN_PRECISION, HLL_MAX_PRECISION);
        let m = 1usize << p;
        Self {
            precision: p,
            registers: vec![0u8; m],
        }
    }

    /// Rebuild from an existing `precision`/`registers` pair (handler entry
    /// decode). Registers shorter/longer than `1 << precision` are zero-padded
    /// or truncated so the invariant always holds.
    pub fn from_registers(precision: u8, mut registers: Vec<u8>) -> Self {
        let p = precision.clamp(HLL_MIN_PRECISION, HLL_MAX_PRECISION);
        let m = 1usize << p;
        registers.resize(m, 0);
        Self {
            precision: p,
            registers,
        }
    }

    pub fn precision(&self) -> u8 {
        self.precision
    }

    pub fn registers(&self) -> &[u8] {
        &self.registers
    }

    /// Documented standard error `1.04/sqrt(m)` for the current register count.
    pub fn standard_error(&self) -> f64 {
        1.04 / (self.registers.len() as f64).sqrt()
    }

    /// Add one element. Returns `true` when a register grew (the estimate may
    /// have changed), matching Redis `PFADD`'s return semantics.
    pub fn add(&mut self, elem: &[u8]) -> bool {
        let p = u32::from(self.precision);
        let h = hll_hash64(elem);
        let idx = (h & ((1u64 << p) - 1)) as usize;
        let w = h >> p;
        // Rank = position (1-based) of the leftmost 1 in the `64 - p` remaining
        // bits; an all-zero tail reports `64 - p + 1`.
        let rank = if w == 0 {
            (65 - p) as u8
        } else {
            (w.leading_zeros() - p + 1) as u8
        };
        if rank > self.registers[idx] {
            self.registers[idx] = rank;
            true
        } else {
            false
        }
    }

    /// Approximate cardinality. Uses the raw harmonic estimate, falling back to
    /// linear counting (`m * ln(m/V)`) while the estimate is small — which keeps
    /// the error well inside `1.04/sqrt(m)` at precision 14.
    pub fn count(&self) -> u64 {
        let m = self.registers.len() as f64;
        let mut sum = 0.0f64;
        let mut zeros = 0usize;
        for &r in &self.registers {
            sum += 2f64.powi(-(i32::from(r)));
            if r == 0 {
                zeros += 1;
            }
        }
        let alpha = match self.registers.len() {
            16 => 0.673,
            32 => 0.697,
            64 => 0.709,
            _ => 0.7213 / (1.0 + 1.079 / m),
        };
        let raw = alpha * m * m / sum;
        if raw <= 2.5 * m && zeros > 0 {
            (m * ((m / zeros as f64).ln())).round() as u64
        } else {
            raw.round() as u64
        }
    }

    /// Register-wise max (union) over the shared buckets. Equal precision gives
    /// the exact union estimate; differing precision keeps self's buckets.
    pub fn merge(&mut self, other: &HyperLogLog) {
        for (a, b) in self.registers.iter_mut().zip(other.registers.iter()) {
            if *b > *a {
                *a = *b;
            }
        }
    }

    /// Zero every register (Redis `PFMERGE` dest reset / L0 clear).
    pub fn clear(&mut self) {
        for r in &mut self.registers {
            *r = 0;
        }
    }
}

/// Deterministic MurmurHash64A (the same hash Redis uses for HyperLogLog),
/// stable across runs and platforms. A fixed algorithm keeps the accuracy bound
/// testable; FNV-1a was rejected because its low-order bits are not uniform
/// enough for a 14-bit bucket index.
fn hll_hash64(bytes: &[u8]) -> u64 {
    const M: u64 = 0xc6a4a7935bd1e995;
    const R: u32 = 47;
    const SEED: u64 = 0xadc83b19;
    let mut h: u64 = SEED ^ (bytes.len() as u64).wrapping_mul(M);
    let mut chunks = bytes.chunks_exact(8);
    for chunk in &mut chunks {
        let mut k = u64::from_le_bytes(chunk.try_into().expect("8-byte chunk"));
        k = k.wrapping_mul(M);
        k ^= k >> R;
        k = k.wrapping_mul(M);
        h ^= k;
        h = h.wrapping_mul(M);
    }
    let rem = chunks.remainder();
    if !rem.is_empty() {
        for (i, b) in rem.iter().enumerate() {
            h ^= u64::from(*b) << (8 * i);
        }
        h = h.wrapping_mul(M);
    }
    h ^= h >> R;
    h = h.wrapping_mul(M);
    h ^= h >> R;
    h
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
    HyperLogLog(HyperLogLog),
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
            "hyperloglog" => L0Store::HyperLogLog(HyperLogLog::default()),
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
            L0Store::HyperLogLog(hll) => {
                // `put` = add one element (the key bytes); the value is ignored,
                // mirroring `bloom_filter`. `get`/`len` report the estimate.
                hll.add(key.as_bytes());
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
            L0Store::HyperLogLog(hll) => Ok(Some(hll.count().to_string().into_bytes())),
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
            L0Store::HyperLogLog(hll) => {
                // `delete` = clear the sketch (Redis `PFMERGE` dest reset).
                hll.clear();
                Ok(true)
            }
        }
    }

    pub fn len(&self) -> usize {
        match &self.store {
            L0Store::HashTable(m) | L0Store::Kv(m) => m.len(),
            L0Store::Vector(v) => v.len(),
            L0Store::BloomFilter { inserts, .. } => *inserts as usize,
            L0Store::HyperLogLog(hll) => hll.count() as usize,
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
    fn creatable_count_is_16() {
        assert_eq!(L0_CREATABLE.len(), 16);
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
        assert_eq!(cat.containers.len(), 16);
    }

    #[test]
    fn hyperloglog_workflow_add_count_clear() {
        let mut cat = L0Catalog::new();
        cat.create("acme", "hll", "hyperloglog", StorageModeChoice::Memory)
            .unwrap();
        let c = cat.get_mut("acme", "hll").unwrap();
        for i in 0..1000 {
            c.put(&format!("elem-{i}"), Vec::new()).unwrap();
        }
        let got = c.get("ignored").unwrap().unwrap();
        let est: f64 = String::from_utf8(got).unwrap().parse().unwrap();
        assert!(
            (est - 1000.0).abs() <= 1000.0 * 0.02,
            "estimate {est} off for 1000 distinct"
        );
        assert!(c.delete("ignored").unwrap());
        assert_eq!(c.len(), 0);
    }
}
