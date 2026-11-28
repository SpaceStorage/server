# Contract: Redis-to-Store Type Mappings

**Feature**: `017-redis-dialect-coverage` | Spec: FR-003, FR-004, FR-017

Mapping between Redis-visible types and SpaceStorage internal datatypes per the abstract type interface (`002`). All Redis commands execute against the shared store; no protocol-private type system.

## Type Mapping Matrix

| Redis-visible type | SpaceStorage internal | Level | Storage notes |
|-------------------|----------------------|-------|---------------|
| `string` | Stored as container entry value type `string` (canonical encoding) | L2 entry | Stored inside `kv_collection` container |
| `string` (HLL) | L0 `hyperloglog` primitive (registered extension) | L0 | Same container, different primitive type |
| `string` (Bitmap) | L0 `bitmap` primitive | L0 | Same container, different primitive type |
| `hash` | L0 `hash_table` primitive, schema per `003` | L0 | Hash-table with field-level operations |
| `list` | L0 `linked_list` or `deque` primitive | L0 | Doubly-linked list indexed by position |
| `set` | L0 `hash_table` primitive (value-less) | L0 | Hash set (values are unit/empty) |
| `zset` | Composite: L0 `skip_list` index + L0 `hash_table` member→score map | L0 + L0 | Ranked index with reverse lookup |
| `stream` | Composite: L0 `ring_buffer` append buffer + L0 `append_segment` append-log | L0 + L0 | Ordered by entry ID, group offsets stored alongside |
| `none` | No entry | N/A | Key does not exist |

Internal type and operation names follow the [`003` type-inventory](../../003-type-system/contracts/type-inventory.md): L0 `hash_table`, `linked_list`/`deque`, `skip_list`, `ring_buffer`, `append_segment`, `bitmap`, and the registered extension `hyperloglog`.

**Container**: All Redis keys live inside a per-namespace L2 container of type `kv_collection` (mode: memory/persistent/hybrid per `003`). One default key-space per namespace.

---

## Entry Lifecycle

### Creation

| Redis command | Creates entry of type |
|--------------|----------------------|
| `SET`, `SETNX`, `SETEX`, etc. | `string` |
| `HSET`, `HMSET`, `HSETNX` | `hash` (on first field) |
| `LPUSH`, `RPUSH`, `LPUSHX`, `RPUSHX` (new key) | `list` |
| `SADD` (new key) | `set` |
| `ZADD` (new key) | `zset` |
| `XADD` (new key) | `stream` |
| `PFADD` (new key) | `hyperloglog` (reports as `string` to client) |
| `SETBIT` on non-existent key | `bitmap` via L0 primitive (reports as `string`) |

### Type Enforcement (WRONGTYPE)

Per FR-003 and FR-005: On type mismatch, return:

```
-WRONGTYPE Operation against a key holding the wrong kind of value
```

Examples of mismatch:
- `GET` on a `hash` key
- `HGET` on a `string` key
- `LPUSH` on a `set` key
- `ZADD` on a `list` key

The `TYPE` command returns the Redis-visible type regardless of internal implementation:
- A `hyperloglog` registers as `string`
- A `bitmap` registers as `string`

---

## HyperLogLog Mapping Details

Per FR-017.

| Redis command | L0 `hyperloglog` operation | Notes |
|--------------|---------------------------|-------|
| `PFADD key element [element ...]` | `add(context, elements)` | Returns 1 if approx cardinality changed |
| `PFCOUNT key [key ...]` | `count(context)` / `merge` + `count` | Single key = direct count; multi-key = merge then count |
| `PFMERGE destkey sourcekey [sourcekey ...]` | `merge(dest, sources)` + `clear` | Dest becomes union of sources |

**Precision**: Default `14` (2^14 = 16384 registers) matching Redis default. Standard error ≈ 0.81%.

**Encoding**: Canonical self-describing representation per `003` FR-042. When read through non-Redis protocol, appears as an HLL sketch, not a string.

---

## Bitmap Mapping Details

Operation names are the registered `003` L0 `bitmap` operations (`set`, `clear`, `test`, `and`, `or`, `xor`, `not`, `cardinality`, `rank`, `select`); see the [`003` type-inventory](../../003-type-system/contracts/type-inventory.md).

| Redis command | L0 `bitmap` operation |
|--------------|----------------------|
| `SETBIT key offset value` | `set(context, offset, value)` |
| `GETBIT key offset` | `test(context, offset)` |
| `BITCOUNT key [start end]` | `cardinality(context, start, end)` |
| `BITPOS key bit [start end]` | `select(context, bit, start, end)` (with `rank`/`test` for clear-bit scans) |
| `BITOP operation destkey key [key ...]` | `and`/`or`/`xor`/`not` -> dest |
| `BITFIELD key [GET type offset]` | `test` over the field's bit range |
| `BITFIELD key [SET type offset value]` | `set` over the field's bit range |
| `BITFIELD key [INCRBY type offset increment]` | `test` + `set` over the field's bit range |

**Overflow handling**: `BITFIELD` supports `WRAP`, `SAT`, `FAIL` modes per Redis 7.4 semantics.

---

## Hash-Field Expiry Mapping

Per FR-002, FR-005, FR-007. The hash-field expiry family maps a per-field deadline onto the `hash` entry's `HashField.expire_at` (data-model §3/§7) — a key's TTL and a field's TTL are independent deadlines.

| Redis command | Store operation | Notes |
|--------------|-----------------|-------|
| `HEXPIRE` / `HPEXPIRE` | set field deadline (sec / ms) | Relative to now; returns count of fields updated |
| `HEXPIREAT` / `HPEXPIREAT` | set field deadline (sec / ms) | Absolute timestamp |
| `HEXPIRETIME` / `HPEXPIRETIME` | read field deadline (sec / ms) | `-1` no TTL, `-2` field/key missing |
| `HTTL` / `HPTTL` | read remaining field TTL (sec / ms) | `-1` no TTL, `-2` field/key missing |
| `HGETEX` | get field(s), optionally adjusting field deadline | `EX`/`PX`/`EXAT`/`PXAT`/`PERSIST` options |
| `HGETDEL` | get field(s) and remove them | No rollback on partial get |

A field read after its deadline behaves as absent while the key remains; the `013` `TTL expiration` job removes the expired field (tombstone), and readers also treat a past deadline as absent (purge-on-read). Changing a field deadline never changes the key deadline, and changing the key deadline never changes field deadlines (except deleting the key drops its fields).

---

## Stream Mapping Details

### Entry ID Format

Redis stream IDs are `<milliseconds-sequence>` pairs:
- `*` → server generates ID using current time + sequence
- `0-*` → millisecond 0, next sequence
- Autogenerated IDs are monotonic per total-order stream

### Ordering Modes

| Mode | Lease required | Behavior |
|------|---------------|----------|
| `total-order` (default) | Yes (`006` leadership lease) | All appends serialized through lease holder; IDs are monotonic cluster-wide |
| `producer-order` | No | Appends accepted locally; IDs use local time + local sequence; monotonic per producer only |

**Creation option**: the mode is fixed when the stream key is created. `XADD` on a missing key and `XGROUP CREATE … MKSTREAM` default to `total-order`. `producer-order` is selected only by the documented `XADD` creation-time option on that first write — a container option surfaced on the stream key's entry, not a new Redis command and not an option accepted on later appends. Once the key exists the mode cannot be changed. Total-order appends present the current `006` leadership-lease epoch; a stale epoch is refused and is not an entry (research R6, constitution XII). Producer-order appends take no lease.

### Consumer Group State

Stored alongside the stream:

| Data | Storage |
|------|---------|
| Group name → last delivered ID | Stream metadata |
| Consumer name → idle time, PEL | Group metadata |
| PEL entries (pending) | In-memory + WAL per `013` (persistent mode) |

### Durability

Per FR-016 (`013` integration):
- Persistent/hybrid streams: Content and consumer offsets survive restart
- Memory streams: Content and groups lost on restart
- Pub/sub: Never persisted (memory only)

---

## Transaction Mapping

Per FR-008.

| Redis concept | Implementation |
|--------------|----------------|
| `MULTI` | Begin command batch in connection state |
| Command inside MULTI | Queued as `TypeOp` or `LogicalRequest`; not executed yet |
| `EXEC` | Execute queue atomically; on error continue (no rollback) |
| `DISCARD` | Clear queue, release WATCH |
| `WATCH key [key ...]` | Register keys for optimistic check; compare version at EXEC |
| `UNWATCH` | Clear watched keys |

**Isolation**: Default `READ COMMITTED` (sees committed changes from other connections). `WATCH` provides optimistic concurrency control.

**No rollback**: If one command in EXEC fails, subsequent commands still execute. This matches Redis semantics.

---

## Pub/Sub Mapping

Per FR-009.

| Redis concept | Implementation |
|--------------|----------------|
| Channel name | String; scoped to namespace |
| Pattern | Glob-style pattern; evaluated at publish time |
| Subscription | Connection enters subscribe mode; yields messages out-of-band |
| Message format | Array: `["message", channel, payload]` or `["pmessage", pattern, channel, payload]` |
| Sharded pub/sub | Accepted but shard ignored (single-node fallback) |

**Delivery guarantees**: Best-effort in-process delivery. No persistence, no replay, no cross-namespace delivery.

---

## Cross-Protocol Visibility

Per FR-003: The same key-space container is visible to other protocols via the canonical mapping:

| Redis key | PostgreSQL view (via `002` mapping) | Notes |
|-----------|-------------------------------------|-------|
| `user:123` (string) | Row in `kv_collection` view | JSONB or text representation |
| `session:data` (hash) | Composite type or JSONB object | Field-value pairs |
| `events` (stream) | Append-only table-like view | Entry ID, fields |
| `metrics:hll` (HLL) | Binary-encoded cardinality sketch | Not human-readable via SQL |

See `002` protocol contracts for full cross-protocol mapping specification.
