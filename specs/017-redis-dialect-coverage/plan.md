# Implementation Plan: Complete-Product Redis Dialect Coverage

**Branch**: `017-redis-dialect-coverage` | **Date**: 2026-11-26 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/017-redis-dialect-coverage/spec.md` (Clarifications, Sessions 2026-10-31 and 2026-11-26 — Redis 7.4-line baseline; resource groups 1–9 plus connection-scoped pub/sub and transactions; single default key-space container; HyperLogLog as registered L0 `hyperloglog`; stream total-order default via a control-plane leadership lease; geo/`XAUTOCLAIM`/Lua/modules/`CLUSTER`/Sentinel/ACL not-supported)

## Summary

Own the **complete-product Redis dialect** as a capability table on top of the narrow first-binary string subset (`016`). Stock Redis clients on RESP2 speak connection/server meta, keyspace introspection, strings and expiry, hashes (including the 7.4 hash-field expiry family), lists, sets, sorted sets, Streams with consumer groups, bitmaps, HyperLogLog, plus connection-scoped pub/sub and transactions. Commands outside that table — the geospatial family, `XAUTOCLAIM`, Lua/`EVAL`/functions, modules, `CLUSTER`, Sentinel roles, ACL-management, and the other families recorded in [contracts/not-supported.md](contracts/not-supported.md) (the canonical closed list) — return the Redis not-supported error form, never a silent empty success.

This feature does **not** add a protocol, a port, a type hierarchy, or an execution engine. It widens `DialectProfile::CompleteProduct` Redis classification in `crates/compat` (the table `015` already says `017` owns), lowers in-table commands through the existing Redis handler (`002`) onto the shared abstract type interface (`003`) and `QueryEngine` (`005`), and registers the one new L0 primitive `hyperloglog` through `003`'s extension path. Every Redis key is a typed entry inside the namespace's single default key-space container. Bitmap commands ride the existing L0 `bitmap`. Stream keys in the default **total-order** model take a `006` leadership lease (constitution XII); **producer-order** streams do not. Durability of stream content and group offsets follows the container's storage mode (`013`). Pub/sub is in-process and fire-and-forget.

**First binary** (`016` slices 1–5) and `HandlersComplete` stay on the `K/V Store` string list. `HGET`, `XADD`, `MULTI`, and the rest of this table remain not-supported there (SC-006 of `016`).

## Technical Context

**Language/Version**: Rust 1.88 (edition 2024, MSRV 1.88), same Cargo workspace as `001`–`016`.

**Primary Dependencies**: existing workspace (`tokio`, `async-trait`, `serde`/`serde_json`, `bytes`, `tracing`, `parking_lot`). RESP codec stays the in-tree `handler-redis` encoder (`002` named `redis-protocol`; the crate already speaks RESP2 without a new dependency). No Lua VM, no Redis module ABI, no `*-sys`, no geospatial library.

**Storage**: No new on-disk format and no second WAL. Redis keys live as typed entries in one per-namespace container (default type L2 `kv_collection`, mode inherited from that container — memory / hybrid / persistent per `003`). String, hash, list, set, and sorted-set entries are values of existing L0/L2 primitives addressed by key. Stream entries and consumer-group state are values whose ordered appends go through `005` and, for total order, a `006` leadership lease; persistent/hybrid content and group offsets restore from the per-drive WAL (`013`). HyperLogLog state is the registered L0 `hyperloglog` value (precision default 14). Bitmap state is the existing L0 `bitmap`. Key and hash-field expiry are TTL records consumed by `013`'s `TTL expiration` job. Pub/sub channels are process memory only.

**Testing**: `cargo test`. Unit: capability-table classify per profile, command-argument errors with no partial write, `WRONGTYPE`, HLL standard-error bound, stream ID rules, `WATCH` abort, `EXEC` no-rollback. `crates/conformance` feature `redis-dialect` (complete-product profile only): stock `redis` client workflows for SC-001–SC-008 — collection families, Streams groups, transactions, pub/sub namespace isolation, `INFO` sections, bitmap/`BITOP`, `PFADD`/`PFCOUNT`/`PFMERGE` through the abstract interface, hash-field expiry, limit rejects, not-supported list (`GEOADD`, `XAUTOCLAIM`, `EVAL`, `CLUSTER`, `SENTINEL`, `ACL`). First-binary conformance stays green: the same verbs still error on `DialectProfile::FirstBinary`. Restart of a persistent stream restores content, last-delivered positions, and group offsets (`013`); memory-mode does not; pub/sub is not replayed.

**Target Platform**: Linux x86_64/aarch64 servers (primary), macOS for development. Multi-connection tests are in-process on ephemeral ports, same harness as `002`/`016`. Total-order stream tests use the in-process `006` lease grant (single node is a trivial epoch); a second connection is enough to prove interleaving and `WATCH`.

**Project Type**: Cargo workspace extension — **no new crate and no new binary**. Widen `crates/compat` Redis classification for `CompleteProduct` only; extend `crates/handler-redis` (command modules, connection state for block/subscribe/MULTI/WATCH, INFO renderer); register `hyperloglog` from `crates/types/src/l0.rs` through `crates/types/src/catalog.rs` + `crates/types/src/descriptor.rs` (FR-046: baseline 55 unchanged); add entry-kind operations on the existing key-space container via `003` `TypeOp` / `005` `LogicalRequest`. Config knob `protocols.redis.max_block` only. Conformance tests live in `crates/conformance`.

**Performance Goals**: Command classify < 1 µs (inherited `015`). A non-blocking point command (GET/HGET/ZSCORE) on the loopback harness stays inside the `002` smoke budget. A blocking command parks its connection task only (`tokio` wait, not a worker-thread sleep) and releases on the triggering write or the effective timeout. `INFO` is a metadata read and does not scan payload bytes beyond the key-space counters it already maintains. HyperLogLog `PFCOUNT` is O(registers) at the key's precision (default 16 384).

**Constraints**: RESP2 only (`015`: RESP3 is a later spec change). Redis 7.4-line semantics for listed families; anything newer or absent from the table is not-supported — nothing in between. One default key-space container per namespace; one Redis-visible type per key; `WRONGTYPE` on mismatch; the container is one shared object other protocols address under the documented mapping (`002`). No protocol-private type and no protocol-private execution engine. Blocking with timeout 0 is indefinite unless `protocols.redis.max_block` is set, in which case that cap wins over 0 and over longer explicit timeouts. Total-order stream writes present the current `006` lease epoch; a stale epoch is refused and is not an entry. Producer-order streams do not take a lease. Pub/sub never crosses a namespace and is never written to the WAL. `SELECT n>0` stays unsupported (`002`). First-binary and `HandlersComplete` classification of these commands stays `MustNot`. Size, connection, and admission limits stay `015`; exceeding one rejects in Redis form before partial execution.

**Scale/Scope**: 1 dialect table (complete product) over 9 resource groups + 2 connection-scoped stories; 6 Redis-visible types; 1 new L0 type (`hyperloglog`) registered as an extension, not a 56th baseline type; 1 per-namespace container; 2 stream order modes; 1 admin block-time knob. Roughly: capability table in `compat` (~1.5 k), command lowerers (~6 k), connection state for block/subscribe/MULTI (~2 k), stream + group ops (~2.5 k), HLL primitive + descriptor (~1 k), INFO/COMMAND renderer (~1 k), conformance (~4 k); ≈ 16–20 k lines including tests.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Gate | Status |
|---|-----------|------|--------|
| I | Rust-Only Stack | Dialect code is pure Rust in existing crates. No Lua runtime, no Redis module loader, no geo library, no `*-sys` | PASS |
| II | Fully Asynchronous Tokio Runtime | Command handling stays `async` on the `001` runtime. A blocking Redis command waits on a `Notify`/channel; it does not sleep a worker thread. Fsync of durable stream/collection writes stays on `013`'s blocking pool. Counted persistent acks wait the durability contract, not the in-memory apply | PASS |
| III | Single-Process Multithreaded Monolith | No new process, sidecar, or binary. Pub/sub fan-out is in-process. Cluster scale-out stays `004`/`011`, not Redis Cluster | PASS |
| IV | Type-Driven Multiparadigm | `hyperloglog` is one L0 primitive registered through `003` FR-045–FR-049. Bitmap commands use existing `bitmap`. Collections and streams are entries in one container of existing types. No parallel Redis type hierarchy | PASS |
| V | Protocol Compatibility on Distinct Ports | The Redis handler and its port stay `002`. This feature is the complete-product verb ceiling `015` FR-016 already assigns here. RESP2 only. No SpaceStorage-native client protocol. First binary remains the narrower `016` subset | PASS |
| VI | Every Node Is a Request Coordinator | Any member accepts the dialect. Total-order stream writes are forwarded to the lease holder (`006`); the receiving node remains the client's coordinator | PASS |
| VII | Label-Based Planetary Placement | Unchanged. The key-space container is placed by `004` like any other container. Stream order is a lease, not a topology domain | PASS |
| VIII | Cassandra-Style Quorum with Protocol Defaults | Redis still has no quorum field. `SS.OPTIONS` / session / global defaults from `002` apply. Write `TWO` counts durable replicas (`013`/`012`). Transactions do not invent a second quorum | PASS |
| IX | Multi-Tenant Namespaces | Credential-bound namespace (`002`). Keys, pub/sub channels, and transactions never cross a namespace. `INFO keyspace` / `DBSIZE` count that namespace's live keys | PASS |
| X | Observability as a Product Surface | `INFO` reads `008` stats; it does not invent a second metric store. Additive Redis command counters use existing `protocol` / `namespace` labels. No required series renamed | PASS |
| XI | Documented, Expandable Configuration | One new knob, `protocols.redis.max_block`, documented with default "unset = stock timeout 0". The capability table is data in `compat`, not a closed language | PASS |
| XII | Raft Controller Elections and Local Restore | Total-order streams **request** a leadership lease; they do not run Raft and do not assume a datatype primary serializes every container. Persistent streams and group offsets restore from `013`. Memory-mode content is not promised. Pub/sub is not restored | PASS |
| XIII | Security Defaults for Data and Roles | AUTH and roles stay `002`/`014`. Redis ACL-management commands are not-supported. Encryption of the key-space container stays per container (`003`/`014`) | PASS |
| Arch. Contracts | Adapter ≠ type system | The handler lowers MUST commands to `LogicalRequest` / `TypeOp` on the abstract interface. MUST NOT never becomes IR. `hyperloglog` is reached only through that interface (`03` FR-040) | PASS |
| Observability Contract | No `08` series renamed | `INFO` maps existing series; omitted Redis fields are omitted or documented, never constant fakes | PASS |

**Gate result (pre-research)**: PASS. The complete-product-only widening of `compat`'s Redis classify (today the core MUST list is identical on every profile) is a planned delta recorded in Complexity Tracking; it does not relax a principle.

## Project Structure

### Documentation (this feature)

```text
specs/017-redis-dialect-coverage/
├── plan.md
├── research.md                     # Phase 0: R1–R14
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── capability-table.md         # FR-001, FR-002, FR-011, FR-012
│   ├── not-supported.md            # FR-012 — canonical closed not-supported list + error form
│   ├── redis-mappings.md           # FR-003, FR-004, FR-017 — type, stream, hash-field, bitmap, HLL mapping
│   └── fixtures/
│       ├── complete-product-redis.conf
│       ├── memory-mode-pubsub.conf
│       └── max-block-negative.conf
├── checklists/requirements.md
└── tasks.md                        # Phase 2 (/speckit-tasks — NOT created here)
```

### Source Code (repository root)

```text
crates/
├── compat/                                  # spacestorage-compat — Redis classify delta
│   └── src/
│       └── matrix.rs                        # CompleteProduct Redis MUST = capability table;
│                                            # FirstBinary and HandlersComplete unchanged
│
├── handler-redis/                           # spacestorage handler — lower + render only
│   └── src/
│       ├── commands/
│       │   ├── mod.rs                       # dispatch after classify(); connection state
│       │   ├── strings.rs                   # GET/SET family + expiry (extends first binary)
│       │   ├── hash.rs                      # H* including HEXPIRE family
│       │   ├── list.rs                      # L* + BLPOP/BRPOP
│       │   ├── set.rs                       # S* + *STORE
│       │   ├── zset.rs                      # Z* + BZPOPMIN/BZPOPMAX
│       │   ├── stream.rs                    # X* except XAUTOCLAIM
│       │   ├── bitmap.rs                    # SETBIT/GETBIT/BITCOUNT/BITPOS/BITOP
│       │   ├── hll.rs                       # PFADD/PFCOUNT/PFMERGE → hyperloglog ops
│       │   ├── txn.rs                       # MULTI/EXEC/DISCARD/WATCH/UNWATCH
│       │   ├── pubsub.rs                    # SUBSCRIBE/PSUBSCRIBE/PUBLISH
│       │   ├── server.rs                    # INFO/DBSIZE/TYPE/SCAN/OBJECT/COMMAND/PING
│       │   └── ss.rs                        # existing SS.* canonical path (002), unchanged
│       ├── lower.rs                         # Redis command → LogicalRequest / TypeOp
│       ├── render.rs                        # RESP2 reply shapes, WRONGTYPE, -ERR
│       ├── block.rs                         # per-connection wait; max_block
│       ├── expiry.rs                        # Deadline/TtlInstant; shares 013 TTL clock
│       └── session.rs                       # subscribe mode, MULTI queue, WATCH set
│
├── types/                                   # descriptor + canonical payload kind
│   └── src/{l0.rs, catalog.rs, descriptor.rs}   # hyperloglog L0 primitive + registration
├── query/                                   # TypeOp dispatch for entry ops; no Redis types
│   └── src/{request.rs, planner.rs, engine.rs}
├── controlplane/                            # LeadershipLease request for total-order streams
├── storage/                                 # TTL job already owns expiry; group offsets in the value
├── config/                                  # protocols.redis.max_block
├── node/                                    # in-process pub/sub bus, namespace-keyed
├── release-profile/                         # redis-dialect tests compile on complete product
└── conformance/
    └── tests/
        ├── redis_collections.rs             # SC-001, SC-005, SC-007 hash-field TTL
        ├── redis_streams.rs                 # SC-003, FR-016 restart
        ├── redis_txn_pubsub.rs              # SC-004, namespace isolation
        ├── redis_info.rs                    # SC-006
        ├── redis_bitmap_hll.rs              # SC-007
        ├── redis_not_supported.rs           # SC-002
        └── redis_limits.rs                  # SC-008
```

**Structure Decision**: Keep the dialect in the handler and the matrix in `compat`. A new `redis-engine` crate would be the protocol-private execution engine constitution IV and `002` FR-035 forbid. Entry operations are `TypeOp`s on the shared key-space container, so PostgreSQL/S3/other protocols see the same object through the canonical mapping (`002`) without importing Redis command code. `hyperloglog` is registered beside `bloom_filter` in `crates/types/src/l0.rs` and listed only under `003` "Registered extensions" — the baseline count stays 55 (`03` FR-046).

**Path reconciliation** (no new crate/binary): earlier drafts used conceptual crate labels; they map to the existing modules that [tasks.md](tasks.md) targets — `crates/l0` → `crates/types/src/l0.rs`; `crates/typeset` → `crates/types/src/catalog.rs` + `crates/types/src/descriptor.rs`; `crates/exec` → `crates/query/src/{request,planner,engine}.rs`. The `crates/handler-redis/src/commands/*.rs`, `lower.rs`, `render.rs`, `session.rs`, and `block.rs` modules are new files added by this feature (the existing `commands.rs` is folded into `commands/mod.rs`).

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| `compat` Redis classify stops being profile-invariant for the core list | `015` FR-016 and this spec FR-015: complete-product MUST is this capability table; first-binary MUST stays the `016` string list. Today `classify_redis` ignores the profile | One list for every profile would either shrink the complete product to GET/SET or make `HGET` succeed on the first binary, contradicting `016` SC-006 |
| Per-key stream ordering mode (total vs producer) instead of one global order | Clarification 2026-10-31: default total order for stock clients; producer order is the explicit alternative | Forcing every stream through a lease would deny the clarified producer-order model; forcing none would break the single ID sequence stock Redis clients rely on |
| Hash-field TTL stored on the entry, not as a second `013` clock | `013` TTL is per value via the expiration job. Hash fields are values inside one key | A parallel field-expiry service would duplicate the tombstone job and could drift from key TTL |
| In-process pub/sub bus with no WAL record | Spec FR-016: pub/sub is fire-and-forget and MUST NOT be persisted or replayed (stock Redis) | Writing publishes to the WAL would replay messages after restart, which stock clients do not expect |
| `INFO` omits or documents fields the product does not have | Spec FR-010: never constant fakes | Emitting stock Redis memory-fragmentation constants would lie to monitoring |

No constitution principle is violated. The rows above are scoped deltas against sibling specs, not principle exceptions.

## Phase 0 — Research

See [research.md](research.md), R1–R14. All Technical Context items are resolved; no `NEEDS CLARIFICATION` remains. Key outcomes: capability table lives in `compat` and is consulted before IR (R1); one `kv_collection` container holds typed entries (R2); collection and stream commands are `TypeOp`s, not a Redis engine (R3); `hyperloglog` registers through `003` and Redis still reports `string` (R4); bitmaps use L0 `bitmap` (R5); total-order streams take a `006` lease, producer-order streams do not (R6); blocking parks one connection task under `max_block` (R7); `MULTI`/`EXEC` is a connection-scoped batch with Redis no-rollback and `WATCH` abort, isolation default `READ COMMITTED` (R8); pub/sub is namespace-local and not durable (R9); `INFO` maps `008` (R10); not-supported list and error form (R11); expiry shares `013` (R12); first-binary profile is frozen (R13); conformance is complete-product only (R14).

## Phase 1 — Design

- [data-model.md](data-model.md): capability-table entry, key-space container, typed entry, stream and consumer group, subscription, transaction session, expiry record, HyperLogLog descriptor, blocking wait, error forms.
- [contracts/](contracts/): [capability-table.md](contracts/capability-table.md), [not-supported.md](contracts/not-supported.md), [redis-mappings.md](contracts/redis-mappings.md), and [fixtures/](contracts/fixtures/) (`complete-product-redis.conf`, `memory-mode-pubsub.conf`, `max-block-negative.conf`).
- [quickstart.md](quickstart.md): complete-product stock-client walkthrough for SC-001–SC-008, plus the first-binary negative check.

## Constitution Check (post-design)

Re-evaluated after Phase 1: every Redis command is classified before IR; MUST commands lower to the shared type interface and `005`; MUST NOT (geo, `XAUTOCLAIM`, Lua, modules, `CLUSTER`, Sentinel, ACL) never becomes IR; `hyperloglog` is one registered L0 type and does not change the baseline 55; bitmaps reuse `bitmap`; the key-space container is one shared object; total-order streams request a lease instead of embedding Raft; fsync stays on the `013` pool; blocking does not stall a worker; pub/sub is not durable and not cross-namespace; `INFO` does not fake series; first-binary classification is unchanged. **Gate result: PASS.**

## Risks and sequencing notes

- **Profile switch, not a rewrite.** `handler-redis` already calls `classify_outcome` before dispatch. Implementation adds complete-product arms; it must not move first-binary verbs (including today's accidental `MustNot` for `INFO`/`COMMAND` on the first binary — those stay not-supported until `CompleteProduct`, matching `016` "off the K/V list").
- **`002` Redis contract is a pointer, not a second table.** `contracts/protocols/redis.md` already says the complete dialect is owned here. Do not copy the command list back into `002`.
- **`003` inventory text is already aligned** (`hyperloglog` under Registered extensions). Implementation registers the type; this plan does not edit `003`.
- **Lease dependency.** Total-order `XADD` on a single node still needs a granted epoch from `006`. If the namespace group has no leader, the command returns an unavailable error in Redis form and writes nothing — it does not fall open into producer order.
- **Order of implementation** (for `/speckit-tasks`): `compat` table and not-supported tests first; key-space entry model and `WRONGTYPE`; strings/expiry already partly in the handler, then hash/list/set/zset; streams and groups; transactions and pub/sub; bitmaps and HLL; `INFO`/`COMMAND`; conformance last, complete-product feature only.

## Next Step

Run `/speckit-tasks` to generate `tasks.md` from this plan and the Phase 1 artifacts. Do not implement in this phase.
