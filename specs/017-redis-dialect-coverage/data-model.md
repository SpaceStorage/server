# Data Model: Complete-Product Redis Dialect

**Feature**: `017-redis-dialect-coverage` | **Date**: 2026-11-26 | **Source**: [spec.md](spec.md) Key Entities, [research.md](research.md)

Types in Rust-ish notation. Owners are named per type. The handler does not own store state except connection-scoped session fields.

## 1. CapabilityCommand (spec: *Redis Capability Table*)

Owned by `crates/compat`. See [contracts/capability-table.md](contracts/capability-table.md).

```text
struct CapabilityCommand {
    name: &'static str,                 // upper-case Redis command
    group: ResourceGroup,
    profile: DialectProfile,           // CompleteProduct for this feature's additions
    options: &'static [&'static str],  // supported options; unknown option → MustNot
}

enum ResourceGroup {
    ConnectionMeta,     // 1
    KeyspaceIntrospection, // 2
    StringExpiry,       // 3
    Hash,               // 4
    List,               // 5
    Set,                // 6
    SortedSet,          // 7
    Stream,             // 8
    Bitmap,             // 9 — also carries the HyperLogLog (PF*) family (capability-table PF* rows are R=9)
    PubSub,             // connection-scoped
    Transaction,        // connection-scoped
}

fn classify_redis(profile: DialectProfile, verb: &str) -> ClassifyOutcome
```

**Invariants**: `FirstBinary` and `HandlersComplete` classify this feature's commands as `MustNot` (research R13). Unknown verb → `MustNot`. `MustNot` never becomes a `LogicalRequest`.

## 2. RedisKeySpace (spec: *Default Key-Space Container*)

Owned by `003` container catalog + `005`. One per namespace.

```text
struct RedisKeySpace {
    namespace: NamespaceName,
    container: ContainerRef,            // default name "redis", schema "public"
    store_type: TypeName,              // L2 kv_collection
    mode: StorageMode,                  // memory | persistent | hybrid — the container's tier
}
```

**Invariants**: Created on first use if the namespace has no container of that name (same auto-create `002` already does). All Redis keys of the namespace live here. Other protocols address this same `ContainerRef`; they do not get a shadow copy. The container's storage mode is the durability tier of every entry in it (spec assumption "persistence follows the container's tier").

## 3. RedisEntry (spec: *Redis-Visible Type*)

Value inside `RedisKeySpace`, keyed by the Redis key (binary-safe).

```text
enum RedisVisibleType { String, Hash, List, Set, ZSet, Stream }

enum EntryPayload {
    Bytes(Vec<u8>),                     // Redis string
    Bitmap(BitmapValue),               // L0 bitmap; TYPE reports String
    HyperLogLog(HllValue),             // L0 hyperloglog; TYPE reports String
    Hash(HashValue),
    List(ListValue),
    Set(SetValue),
    ZSet(ZSetValue),
    Stream(StreamValue),
}

struct RedisEntry {
    key: Bytes,
    visible: RedisVisibleType,
    payload: EntryPayload,
    expire_at: Option<Deadline>,        // key TTL; None = persistent
    version: u64,                       // bumped on every mutation; WATCH reads this
}
```

**Invariants**: `visible` is a function of `payload` (`Bitmap` and `HyperLogLog` → `String`). The first write sets both. A command whose expected visible type differs → `WrongType` and no mutation (version unchanged). Key expiry in the past makes every read observe the key as missing.

### Entry payloads

```text
struct HashValue {
    fields: Map<Bytes, HashField>,
}
struct HashField {
    value: Bytes,
    expire_at: Option<Deadline>,        // HEXPIRE family; None = no field TTL
}

struct ListValue { elements: Deque<Bytes> }          // LPUSH/RPUSH ends

struct SetValue { members: Set<Bytes> }

struct ZSetValue {
    by_member: Map<Bytes, f64>,         // score
    by_score: Ordered<(f64, Bytes)>,    // score, then member lexicographic
}
```

Score ordering is numeric, ties broken by member byte order. `NaN` scores are a command error and do not insert.

## 4. StreamValue and ConsumerGroup (spec: *Stream Consumer Group*)

```text
enum StreamOrder { Total, Producer }    // default Total, fixed at key creation

struct StreamId { ms: u64, seq: u64 }   // Redis "<ms>-<seq>"

struct StreamEntry {
    id: StreamId,
    fields: Vec<(Bytes, Bytes)>,        // name/value pairs, order preserved
}

struct StreamValue {
    order: StreamOrder,
    entries: OrderedMap<StreamId, StreamEntry>,
    last_id: StreamId,
    groups: Map<Bytes, ConsumerGroup>,  // group name
    lease: Option<LeaseEpoch>,          // Some iff order == Total
}

struct ConsumerGroup {
    name: Bytes,
    last_delivered: StreamId,           // XGROUP CREATE id, then advanced by reads
    pending: Map<StreamId, PendingEntry>,
}

struct PendingEntry {
    id: StreamId,
    consumer: Bytes,
    delivery_count: u64,
    delivered_at: Timestamp,            // idle time = now - delivered_at
    body: StreamEntry,
}
```

**Invariants**:

- IDs strictly increase for `*` generation. An explicit ID ≤ `last_id` is rejected and appends nothing.
- `Total`: append presents `lease.epoch`. Stale epoch → error, entry not stored (`006`).
- `Producer`: `lease` is `None`. Per-producer ID order holds; cross-producer interleaving is allowed.
- `XREADGROUP` delivers each entry to at most one consumer at a time (it is in that consumer's PEL). `XACK` removes it from `pending`. `XCLAIM` moves it, increments `delivery_count`, and does not change the entry body.
- `XAUTOCLAIM` is not representable here; the handler rejects it before this struct is touched.
- Trim (`XTRIM` / `MAXLEN` / `~`) drops oldest IDs only. Surviving entries keep id and body. Pending records whose entry was trimmed follow Redis 7.4 trim rules (PEL entries for deleted IDs are removed).
- Persistent/hybrid: `entries`, `last_id`, `groups.last_delivered`, and `pending` are inside the container value and therefore in the `013` WAL. Memory mode: none of this is promised across process restart. Pub/sub is not in this struct.

## 5. Subscription (spec: *Subscription*, *Namespace-Scoped Channel Name*)

Owned by connection state + `node::PubSubBus`. Not stored in the key-space container.

```text
struct Subscription {
    connection: ConnectionId,
    namespace: NamespaceId,             // credential-bound; immutable for the connection
    channels: Set<Bytes>,
    patterns: Set<Bytes>,               // glob-style, Redis PSUBSCRIBE rules
}

struct PubSubMessage {
    namespace: NamespaceId,
    channel: Bytes,
    payload: Bytes,
}
```

**Invariants**: Deliver only when `message.namespace == subscription.namespace`. Pattern delivery includes the original channel and the matched pattern in the RESP array. No message is written to the WAL. Restart drops every `Subscription`. `PUBLISH` returns the count of matching subscriptions on **this process**.

## 6. TransactionSession (spec: *Transaction Session*)

Connection-local. Not a `005` rollback transaction.

```text
enum TxnState { Idle, Queuing { dirty: bool }, Executing }

struct TransactionSession {
    state: TxnState,
    queue: Vec<QueuedCommand>,
    watched: Map<Bytes, u64>,           // key → RedisEntry.version at WATCH time
}

struct QueuedCommand {
    argv: Vec<Bytes>,
    lowered: LogicalRequest,            // built at queue time
}
```

**Invariants**:

- `dirty == true` (queuing-time arity/syntax error) ⇒ `EXEC` runs nothing and returns `EXECABORT`.
- `EXEC` when any watched key is missing or its `version` differs ⇒ null bulk, queue discarded, nothing runs.
- Otherwise each queued command runs in order. An execution error is one element of the reply array; previous successful writes remain; subsequent commands run.
- `WATCH` outside `MULTI` is held until `EXEC`, `DISCARD`, `UNWATCH`, or disconnect.
- Blocking commands, `SUBSCRIBE`, and nested `MULTI` in the queue are queuing errors (`dirty`).

Isolation attached to each lowered request is `READ COMMITTED` unless the session `SS.OPTIONS` path has set something `005` already allows. `SERIALIZABLE` is not offered.

## 7. KeyExpiry / FieldExpiry (spec: *Key Expiry Record*)

```text
struct Deadline {
    at: TtlInstant,                     // 013 TTL clock: event-time if present, else source-domain ingest/HLC
}
```

Lives on `RedisEntry.expire_at` and `HashField.expire_at`. The `013` `TTL expiration` job deletes an expired key (tombstone) or removes an expired field. Readers also treat `at <= now` as missing so a late job cannot resurrect a value.

**Invariants**: `TTL`/`PTTL` report remaining time or `-1` (no expiry) or `-2` (missing). `PERSIST` clears `expire_at`. Field commands never change the key deadline, and key commands never change field deadlines, except deleting the key drops its fields.

## 8. HyperLogLogDescriptor and HllValue (registered L0 `hyperloglog`; spec: *HyperLogLog Sketch Type*)

Owned by `crates/types/src/l0.rs` (the L0 primitive module), registered by `crates/types/src/catalog.rs` + `crates/types/src/descriptor.rs`. Descriptor matches `003` Registered extensions. This feature does not add fields the `003` catalog does not already require.

```text
struct HyperLogLogDescriptor {
    name: "hyperloglog",
    level: L0,
    kind: DataStructure,
    creatable: true,
    modes: { memory: default, persistent, hybrid },
    schema: Optional { precision: u8 }, // default 14, range 4..=16
    operations: ["add", "count", "merge", "clear"],
    standard_error: "1.04/sqrt(2^precision)",
    canonical_version: 1,
}

struct HllValue {
    precision: u8,                      // frozen at PFADD create
    registers: /* 2^precision counters */,
}
```

**Invariants**: Baseline type count stays 55 (`03` FR-046). `PFMERGE` creates the destination at the maximum precision of its inputs when the destination is missing. A `PFADD` against a key whose visible type is not `string`, or whose string payload is not `HyperLogLog`, is `WRONGTYPE`. Direct container create of `hyperloglog` (outside Redis) remains allowed because every L0 data structure is creatable (`03` FR-014); Redis `PF*` does not require the client to create that container — the sketch is an **entry payload** inside the key-space container. The catalog type exists so the op set and canonical form are real (`03` FR-040, FR-049).

## 9. BlockingWait

Connection-local. Not durable.

```text
enum BlockKind { BlPop, BrPop, BzPopMin, BzPopMax, XRead, XReadGroup }  // XReadGroup: XREADGROUP … BLOCK

struct BlockingWait {
    connection: ConnectionId,
    keys: Vec<Bytes>,
    kind: BlockKind,                    // BlPop | BrPop | BzPopMin | BzPopMax | XRead | XReadGroup
    deadline: Option<Instant>,          // None only when max_block is unset AND client timeout is 0
}

struct RedisProtocolConfig {
    max_block: Option<Duration>,        // None = stock indefinite timeout 0
}
```

**Invariants**: At most one `BlockingWait` per connection. Dropped on reply, timeout, disconnect, or drain. Effective timeout = client timeout, overridden by `max_block` when `max_block` is set and (client is 0 or client > max_block).

## 10. Connection session (assembled)

```text
struct RedisSession {
    namespace: NamespaceName,           // from AUTH, 002
    container: ContainerRef,            // the key-space
    authenticated: bool,
    txn: TransactionSession,
    subscriptions: Option<Subscription>, // Some ⇒ subscribe mode
    blocking: Option<BlockingWait>,
    options: QueryOptions,              // quorum/timeout, 002 SS.OPTIONS
}
```

## Relationships

```text
Namespace 1──1 RedisKeySpace
RedisKeySpace 1──* RedisEntry
RedisEntry 1──1 EntryPayload
RedisEntry 0..1 Deadline
HashValue 1──* HashField
HashField 0..1 Deadline
StreamValue 1──* StreamEntry
StreamValue 0..1 LeaseEpoch          (iff Total)
StreamValue 1──* ConsumerGroup
ConsumerGroup 1──* PendingEntry
RedisSession 0..1 TransactionSession
RedisSession 0..1 Subscription
RedisSession 0..1 BlockingWait
Subscription *──1 Namespace          (never another namespace)
PubSubMessage *──1 Namespace
HyperLogLog descriptor ──registered by── crates/types/src/catalog.rs (extension, not baseline)
```

## Validation and errors

Wire forms: the not-supported commands and their error form are canonical in [contracts/not-supported.md](contracts/not-supported.md); the remaining forms below follow the `002` Redis renderer.

| Condition | Result | State change |
|-----------|--------|----------------|
| Arity mismatch | `-ERR wrong number of arguments for '<cmd>'` | none |
| Bad score, bad ID, bad integer | `-ERR` naming the argument | none |
| Command or option not in the table | `-ERR unknown command '<cmd>'` | none |
| Visible-type mismatch | `-WRONGTYPE Operation against a key holding the wrong kind of value` | none |
| Limit (`015`) | `-ERR` naming the limit | none |
| Total-order stale epoch or no lease holder | `-ERR` unavailable, command name | none |
| `WATCH` conflict at `EXEC` | null bulk (`*-1` / RESP2 null array) | queue dropped |
| Queuing error then `EXEC` | `-EXECABORT` | queue dropped, nothing ran |
| Expired key on read | missing-key reply for that command | lazy purge may delete |

## State transitions

### Entry

```text
(absent) --first write--> typed entry
typed entry --same-type write--> typed entry (version++)
typed entry --other-type command--> WRONGTYPE (unchanged)
typed entry --expire or DEL--> absent
```

### Transaction

```text
Idle --MULTI--> Queuing{dirty:false}
Queuing --bad arity--> Queuing{dirty:true}
Queuing --DISCARD--> Idle (empty)
Queuing{dirty:true} --EXEC--> Idle (EXECABORT, no writes)
Queuing{dirty:false} --EXEC, watch ok--> Executing --> Idle
Queuing --EXEC, watch fail--> Idle (null, no writes)
```

### Subscription

```text
command mode --SUBSCRIBE|PSUBSCRIBE--> subscribe mode
subscribe mode --UNSUBSCRIBE all and PUNSUBSCRIBE all--> command mode
disconnect --> dropped (no replay)
```

### Blocking

```text
running --BLPOP timeout--> parked
parked --push | deadline | disconnect | drain--> running
```
