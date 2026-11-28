//! Key-space entry model for the complete-product dialect (`017` T006,
//! data-model §2/§3). Every Redis key is one typed entry inside the namespace's
//! single default container (`kv_collection`); the store owns the canonical
//! L0/L2 value, this model carries the Redis-visible kind and the commands'
//! working view.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use bytes::Bytes;
use spacestorage_types::{NamespaceName, StorageModeChoice};

use crate::expiry::Deadline;

/// Redis-visible type reported by `TYPE` (FR-003/FR-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedisVisibleType {
    String,
    Hash,
    List,
    Set,
    ZSet,
    Stream,
}

impl RedisVisibleType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Hash => "hash",
            Self::List => "list",
            Self::Set => "set",
            Self::ZSet => "zset",
            Self::Stream => "stream",
        }
    }
}

/// One key-space container per namespace (data-model §2): default name
/// `"redis"`, schema `"public"`, store type L2 `kv_collection`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedisKeySpace {
    pub namespace: NamespaceName,
    pub container: String,
    pub store_type: &'static str,
    pub mode: StorageModeChoice,
}

impl RedisKeySpace {
    pub fn new(namespace: NamespaceName, container: impl Into<String>, mode: StorageModeChoice) -> Self {
        Self {
            namespace,
            container: container.into(),
            store_type: "kv_collection",
            mode,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HashField {
    pub value: Bytes,
    /// `HEXPIRE` family; `None` = no field TTL.
    pub expire_at: Option<Deadline>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct HashValue {
    pub fields: BTreeMap<Bytes, HashField>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ListValue {
    pub elements: VecDeque<Bytes>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SetValue {
    pub members: BTreeSet<Bytes>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ZSetValue {
    /// member → score. Score ordering is numeric, ties broken by member bytes.
    pub by_member: BTreeMap<Bytes, f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct StreamId {
    pub ms: u64,
    pub seq: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StreamEntry {
    pub id: StreamId,
    pub fields: Vec<(Bytes, Bytes)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamOrder {
    Total,
    Producer,
}

/// One pending (unacknowledged) delivery of a stream entry to a consumer
/// (data-model §4). `delivered_at` is Unix milliseconds; idle = now − delivered_at.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingEntry {
    pub id: StreamId,
    pub consumer: Bytes,
    pub delivery_count: u64,
    pub delivered_at: i64,
    /// The entry body at delivery time (kept so `XCLAIM` returns it even if the
    /// entry was later trimmed away from the stream — data-model §4).
    pub body: StreamEntry,
}

/// A named position and delivery state over one stream key (data-model §4).
#[derive(Debug, Clone, PartialEq)]
pub struct ConsumerGroup {
    pub name: Bytes,
    /// `XGROUP CREATE` id, then advanced by `XREADGROUP` `>` reads.
    pub last_delivered: StreamId,
    /// Known consumers, including ones created by `XGROUP CREATECONSUMER` with
    /// no pending entries (Redis tracks consumers separately from the PEL).
    pub consumers: BTreeSet<Bytes>,
    pub pending: BTreeMap<StreamId, PendingEntry>,
}

impl ConsumerGroup {
    pub fn new(name: Bytes, last_delivered: StreamId) -> Self {
        Self {
            name,
            last_delivered,
            consumers: BTreeSet::new(),
            pending: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StreamValue {
    pub order: StreamOrder,
    pub entries: BTreeMap<StreamId, StreamEntry>,
    pub last_id: StreamId,
    /// `Some` iff `order == Total` (`006` leadership lease epoch).
    pub lease: Option<u64>,
    /// Group name → group state (`XGROUP` family, data-model §4).
    pub groups: BTreeMap<Bytes, ConsumerGroup>,
}

impl StreamValue {
    /// A fresh total-order stream with no entries (default model).
    pub fn new_total(lease: u64) -> Self {
        Self {
            order: StreamOrder::Total,
            entries: BTreeMap::new(),
            last_id: StreamId { ms: 0, seq: 0 },
            lease: Some(lease),
            groups: BTreeMap::new(),
        }
    }

    /// A fresh producer-order stream with no entries (creation-time option).
    pub fn new_producer() -> Self {
        Self {
            order: StreamOrder::Producer,
            entries: BTreeMap::new(),
            last_id: StreamId { ms: 0, seq: 0 },
            lease: None,
            groups: BTreeMap::new(),
        }
    }
}

/// Handler-side canonical view of the L0 `bitmap` entry. The store owns the
/// actual primitive; commands reach it through the shared abstract interface.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BitmapValue(pub Vec<u8>);

/// Handler-side canonical view of the registered L0 `hyperloglog` entry.
#[derive(Debug, Clone, PartialEq)]
pub struct HllValue {
    /// Frozen at `PFADD` create; default 14.
    pub precision: u8,
    pub registers: Vec<u8>,
}

/// The payload of a [`RedisEntry`]. `visible` is a pure function of this.
#[derive(Debug, Clone, PartialEq)]
pub enum EntryPayload {
    Bytes(Vec<u8>),
    Bitmap(BitmapValue),
    HyperLogLog(HllValue),
    Hash(HashValue),
    List(ListValue),
    Set(SetValue),
    ZSet(ZSetValue),
    Stream(StreamValue),
}

impl EntryPayload {
    /// `Bitmap` and `HyperLogLog` report `String` (FR-004/FR-017).
    pub fn visible(&self) -> RedisVisibleType {
        match self {
            Self::Bytes(_) | Self::Bitmap(_) | Self::HyperLogLog(_) => RedisVisibleType::String,
            Self::Hash(_) => RedisVisibleType::Hash,
            Self::List(_) => RedisVisibleType::List,
            Self::Set(_) => RedisVisibleType::Set,
            Self::ZSet(_) => RedisVisibleType::ZSet,
            Self::Stream(_) => RedisVisibleType::Stream,
        }
    }
}

/// One Redis key inside [`RedisKeySpace`].
#[derive(Debug, Clone, PartialEq)]
pub struct RedisEntry {
    pub key: Bytes,
    pub visible: RedisVisibleType,
    pub payload: EntryPayload,
    /// key TTL; `None` = persistent.
    pub expire_at: Option<Deadline>,
    /// bumped on every mutation; `WATCH` reads it.
    pub version: u64,
}

impl RedisEntry {
    /// First write: `visible` derives from `payload`.
    pub fn new(key: Bytes, payload: EntryPayload) -> Self {
        let visible = payload.visible();
        Self {
            key,
            visible,
            payload,
            expire_at: None,
            version: 1,
        }
    }

    /// Same-type write bumps `version` and keeps `visible` in sync.
    pub fn replace(&mut self, payload: EntryPayload) {
        self.visible = payload.visible();
        self.payload = payload;
        self.version = self.version.wrapping_add(1);
    }

    /// `true` when the expected Redis-visible type matches (`WRONGTYPE` guard).
    pub fn is_type(&self, expected: RedisVisibleType) -> bool {
        self.visible == expected
    }
}

/// Payload discriminant stored in the first byte of a canonical entry blob.
/// Distinct from [`RedisVisibleType`] because `Bitmap`/`HyperLogLog` report as
/// `String` while keeping their own canonical primitive form.
const KIND_BYTES: u8 = 0;
const KIND_BITMAP: u8 = 1;
const KIND_HLL: u8 = 2;
const KIND_HASH: u8 = 3;
const KIND_LIST: u8 = 4;
const KIND_SET: u8 = 5;
const KIND_ZSET: u8 = 6;
const KIND_STREAM: u8 = 7;

fn payload_kind(payload: &EntryPayload) -> u8 {
    match payload {
        EntryPayload::Bytes(_) => KIND_BYTES,
        EntryPayload::Bitmap(_) => KIND_BITMAP,
        EntryPayload::HyperLogLog(_) => KIND_HLL,
        EntryPayload::Hash(_) => KIND_HASH,
        EntryPayload::List(_) => KIND_LIST,
        EntryPayload::Set(_) => KIND_SET,
        EntryPayload::ZSet(_) => KIND_ZSET,
        EntryPayload::Stream(_) => KIND_STREAM,
    }
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_bytes(out: &mut Vec<u8>, b: &[u8]) {
    put_u32(out, b.len() as u32);
    out.extend_from_slice(b);
}

fn take_u32(buf: &[u8], pos: &mut usize) -> Option<u32> {
    let end = *pos + 4;
    if end > buf.len() {
        return None;
    }
    let v = u32::from_le_bytes(buf[*pos..end].try_into().ok()?);
    *pos = end;
    Some(v)
}

fn take_u64(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let end = *pos + 8;
    if end > buf.len() {
        return None;
    }
    let v = u64::from_le_bytes(buf[*pos..end].try_into().ok()?);
    *pos = end;
    Some(v)
}

fn take_i64(buf: &[u8], pos: &mut usize) -> Option<i64> {
    let end = *pos + 8;
    if end > buf.len() {
        return None;
    }
    let v = i64::from_le_bytes(buf[*pos..end].try_into().ok()?);
    *pos = end;
    Some(v)
}

fn take_bytes(buf: &[u8], pos: &mut usize) -> Option<Bytes> {
    let len = take_u32(buf, pos)? as usize;
    let end = *pos + len;
    if end > buf.len() {
        return None;
    }
    let b = Bytes::copy_from_slice(&buf[*pos..end]);
    *pos = end;
    Some(b)
}

/// Canonical binary encoding of one [`RedisEntry`] as stored in the shared
/// namespace key-space container (`objects` row bytes). Binary-safe.
pub fn encode_entry(entry: &RedisEntry) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(payload_kind(&entry.payload));
    out.extend_from_slice(&entry.version.to_le_bytes());
    let expire = entry
        .expire_at
        .map(|d| d.at.millis)
        .unwrap_or(i64::MIN);
    out.extend_from_slice(&expire.to_le_bytes());
    match &entry.payload {
        EntryPayload::Bytes(b) => put_bytes(&mut out, b),
        EntryPayload::Hash(h) => {
            put_u32(&mut out, h.fields.len() as u32);
            for (k, f) in &h.fields {
                put_bytes(&mut out, k);
                put_bytes(&mut out, &f.value);
                match f.expire_at {
                    Some(d) => {
                        out.push(1);
                        out.extend_from_slice(&d.at.millis.to_le_bytes());
                    }
                    None => out.push(0),
                }
            }
        }
        EntryPayload::List(l) => {
            put_u32(&mut out, l.elements.len() as u32);
            for e in &l.elements {
                put_bytes(&mut out, e);
            }
        }
        EntryPayload::Set(s) => {
            put_u32(&mut out, s.members.len() as u32);
            for m in &s.members {
                put_bytes(&mut out, m);
            }
        }
        EntryPayload::ZSet(z) => {
            put_u32(&mut out, z.by_member.len() as u32);
            for (m, score) in &z.by_member {
                put_bytes(&mut out, m);
                out.extend_from_slice(&score.to_le_bytes());
            }
        }
        EntryPayload::Bitmap(b) => put_bytes(&mut out, &b.0),
        EntryPayload::HyperLogLog(h) => {
            out.push(h.precision);
            put_bytes(&mut out, &h.registers);
        }
        EntryPayload::Stream(s) => encode_stream(&mut out, s),
    }
    out
}

fn put_stream_id(out: &mut Vec<u8>, id: StreamId) {
    put_u64(out, id.ms);
    put_u64(out, id.seq);
}

fn put_stream_entry(out: &mut Vec<u8>, e: &StreamEntry) {
    put_stream_id(out, e.id);
    put_u32(out, e.fields.len() as u32);
    for (f, v) in &e.fields {
        put_bytes(out, f);
        put_bytes(out, v);
    }
}

/// Stream payload codec (`017` T040): content, `last_id`, group
/// `last_delivered`/consumers/`pending` — all inside the entry blob so the
/// existing Persistent/Hybrid durable hook persists it unchanged.
fn encode_stream(out: &mut Vec<u8>, s: &StreamValue) {
    out.push(match s.order {
        StreamOrder::Total => 0,
        StreamOrder::Producer => 1,
    });
    match s.lease {
        Some(e) => {
            out.push(1);
            put_u64(out, e);
        }
        None => out.push(0),
    }
    put_stream_id(out, s.last_id);
    put_u32(out, s.entries.len() as u32);
    for e in s.entries.values() {
        put_stream_entry(out, e);
    }
    put_u32(out, s.groups.len() as u32);
    for (name, g) in &s.groups {
        put_bytes(out, name);
        put_stream_id(out, g.last_delivered);
        put_u32(out, g.consumers.len() as u32);
        for c in &g.consumers {
            put_bytes(out, c);
        }
        put_u32(out, g.pending.len() as u32);
        for p in g.pending.values() {
            put_stream_id(out, p.id);
            put_bytes(out, &p.consumer);
            put_u64(out, p.delivery_count);
            out.extend_from_slice(&p.delivered_at.to_le_bytes());
            put_stream_entry(out, &p.body);
        }
    }
}

/// Decode a canonical entry blob. Returns `None` on a malformed/unknown blob.
pub fn decode_entry(key: Bytes, buf: &[u8]) -> Option<RedisEntry> {
    if buf.len() < 17 {
        return None;
    }
    let kind = buf[0];
    let version = u64::from_le_bytes(buf[1..9].try_into().ok()?);
    let expire_ms = i64::from_le_bytes(buf[9..17].try_into().ok()?);
    let expire_at = if expire_ms == i64::MIN {
        None
    } else {
        Some(Deadline::new(crate::expiry::TtlInstant::from_unix_millis(expire_ms)))
    };
    let mut pos = 17usize;
    let payload = match kind {
        KIND_BYTES => EntryPayload::Bytes(take_bytes(buf, &mut pos)?.to_vec()),
        KIND_BITMAP => EntryPayload::Bitmap(BitmapValue(take_bytes(buf, &mut pos)?.to_vec())),
        KIND_HLL => {
            let precision = *buf.get(pos)?;
            pos += 1;
            let registers = take_bytes(buf, &mut pos)?.to_vec();
            EntryPayload::HyperLogLog(HllValue {
                precision,
                registers,
            })
        }
        KIND_HASH => {
            let n = take_u32(buf, &mut pos)?;
            let mut fields = BTreeMap::new();
            for _ in 0..n {
                let k = take_bytes(buf, &mut pos)?;
                let v = take_bytes(buf, &mut pos)?;
                let has_exp = *buf.get(pos)?;
                pos += 1;
                let expire = if has_exp == 1 {
                    if pos + 8 > buf.len() {
                        return None;
                    }
                    let ms = i64::from_le_bytes(buf[pos..pos + 8].try_into().ok()?);
                    pos += 8;
                    Some(Deadline::new(crate::expiry::TtlInstant::from_unix_millis(ms)))
                } else {
                    None
                };
                fields.insert(k, HashField { value: v, expire_at: expire });
            }
            EntryPayload::Hash(HashValue { fields })
        }
        KIND_LIST => {
            let n = take_u32(buf, &mut pos)?;
            let mut elements = VecDeque::new();
            for _ in 0..n {
                elements.push_back(take_bytes(buf, &mut pos)?);
            }
            EntryPayload::List(ListValue { elements })
        }
        KIND_SET => {
            let n = take_u32(buf, &mut pos)?;
            let mut members = BTreeSet::new();
            for _ in 0..n {
                members.insert(take_bytes(buf, &mut pos)?);
            }
            EntryPayload::Set(SetValue { members })
        }
        KIND_ZSET => {
            let n = take_u32(buf, &mut pos)?;
            let mut by_member = BTreeMap::new();
            for _ in 0..n {
                let m = take_bytes(buf, &mut pos)?;
                if pos + 8 > buf.len() {
                    return None;
                }
                let score = f64::from_le_bytes(buf[pos..pos + 8].try_into().ok()?);
                pos += 8;
                by_member.insert(m, score);
            }
            EntryPayload::ZSet(ZSetValue { by_member })
        }
        KIND_STREAM => EntryPayload::Stream(decode_stream(buf, &mut pos)?),
        _ => return None,
    };
    let visible = payload.visible();
    Some(RedisEntry {
        key,
        visible,
        payload,
        expire_at,
        version,
    })
}

/// Decode a stream payload (`KIND_STREAM`).
fn decode_stream(buf: &[u8], pos: &mut usize) -> Option<StreamValue> {
    let order = match *buf.get(*pos)? {
        0 => StreamOrder::Total,
        1 => StreamOrder::Producer,
        _ => return None,
    };
    *pos += 1;
    let lease = match *buf.get(*pos)? {
        0 => {
            *pos += 1;
            None
        }
        1 => {
            *pos += 1;
            Some(take_u64(buf, pos)?)
        }
        _ => return None,
    };
    let last_id = StreamId {
        ms: take_u64(buf, pos)?,
        seq: take_u64(buf, pos)?,
    };
    let n = take_u32(buf, pos)?;
    let mut entries = BTreeMap::new();
    for _ in 0..n {
        let e = decode_stream_entry(buf, pos)?;
        entries.insert(e.id, e);
    }
    let gn = take_u32(buf, pos)?;
    let mut groups = BTreeMap::new();
    for _ in 0..gn {
        let name = take_bytes(buf, pos)?;
        let last_delivered = StreamId {
            ms: take_u64(buf, pos)?,
            seq: take_u64(buf, pos)?,
        };
        let cn = take_u32(buf, pos)?;
        let mut consumers = BTreeSet::new();
        for _ in 0..cn {
            consumers.insert(take_bytes(buf, pos)?);
        }
        let pn = take_u32(buf, pos)?;
        let mut pending = BTreeMap::new();
        for _ in 0..pn {
            let id = StreamId {
                ms: take_u64(buf, pos)?,
                seq: take_u64(buf, pos)?,
            };
            let consumer = take_bytes(buf, pos)?;
            let delivery_count = take_u64(buf, pos)?;
            let delivered_at = take_i64(buf, pos)?;
            let body = decode_stream_entry(buf, pos)?;
            pending.insert(
                id,
                PendingEntry {
                    id,
                    consumer,
                    delivery_count,
                    delivered_at,
                    body,
                },
            );
        }
        groups.insert(
            name.clone(),
            ConsumerGroup {
                name,
                last_delivered,
                consumers,
                pending,
            },
        );
    }
    Some(StreamValue {
        order,
        entries,
        last_id,
        lease,
        groups,
    })
}

fn decode_stream_entry(buf: &[u8], pos: &mut usize) -> Option<StreamEntry> {
    let id = StreamId {
        ms: take_u64(buf, pos)?,
        seq: take_u64(buf, pos)?,
    };
    let n = take_u32(buf, pos)?;
    let mut fields = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let f = take_bytes(buf, pos)?;
        let v = take_bytes(buf, pos)?;
        fields.push((f, v));
    }
    Some(StreamEntry { id, fields })
}

/// Glob matching for `SCAN`/`HSCAN`/`SSCAN`/`ZSCAN` `MATCH` (`*` and `?`).
pub(crate) fn glob_match(pat: &str, key: &str) -> bool {
    if !pat.contains('*') && !pat.contains('?') {
        return pat == key;
    }
    let pb = pat.as_bytes();
    let kb = key.as_bytes();
    let (mut pi, mut ki) = (0usize, 0usize);
    let (mut star_p, mut star_k) = (None, None);
    while ki < kb.len() {
        if pi < pb.len() && (pb[pi] == b'?' || pb[pi] == kb[ki]) {
            pi += 1;
            ki += 1;
        } else if pi < pb.len() && pb[pi] == b'*' {
            star_p = Some(pi);
            star_k = Some(ki);
            pi += 1;
        } else if let (Some(sp), Some(sk)) = (star_p, star_k) {
            pi = sp + 1;
            ki = sk + 1;
            star_k = Some(ki);
        } else {
            return false;
        }
    }
    while pi < pb.len() && pb[pi] == b'*' {
        pi += 1;
    }
    pi == pb.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_is_function_of_payload() {
        assert_eq!(
            EntryPayload::Bitmap(BitmapValue(vec![0x01])).visible(),
            RedisVisibleType::String
        );
        assert_eq!(
            EntryPayload::HyperLogLog(HllValue {
                precision: 14,
                registers: vec![0; 16],
            })
            .visible(),
            RedisVisibleType::String
        );
        assert_eq!(EntryPayload::Hash(HashValue::default()).visible(), RedisVisibleType::Hash);
        assert_eq!(EntryPayload::List(ListValue::default()).visible(), RedisVisibleType::List);
        assert_eq!(EntryPayload::Set(SetValue::default()).visible(), RedisVisibleType::Set);
        assert_eq!(EntryPayload::ZSet(ZSetValue::default()).visible(), RedisVisibleType::ZSet);
    }

    #[test]
    fn version_bumps_on_mutation() {
        let key = Bytes::from_static(b"k");
        let mut e = RedisEntry::new(key, EntryPayload::Bytes(b"v".to_vec()));
        assert_eq!(e.version, 1);
        e.replace(EntryPayload::Bytes(b"v2".to_vec()));
        assert_eq!(e.version, 2);
        assert_eq!(e.visible, RedisVisibleType::String);
    }

    #[test]
    fn keyspace_defaults() {
        let ks = RedisKeySpace::new(NamespaceName::new("demo"), "redis", StorageModeChoice::Persistent);
        assert_eq!(ks.store_type, "kv_collection");
        assert_eq!(ks.container, "redis");
    }
}
