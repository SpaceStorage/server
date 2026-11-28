---
description: "Task list for complete-product Redis dialect coverage"
---

# Tasks: Complete-Product Redis Dialect Coverage

**Input**: Design documents from `/specs/017-redis-dialect-coverage/`

**Prerequisites**: [plan.md](plan.md) (required), [spec.md](spec.md) (required for user stories), [research.md](research.md) (R1–R14), [data-model.md](data-model.md), [contracts/](contracts/) ([capability-table.md](contracts/capability-table.md), [not-supported.md](contracts/not-supported.md), [redis-mappings.md](contracts/redis-mappings.md)), [quickstart.md](quickstart.md)

**Tests**: Requested. The spec's Independent Tests and SC-001–SC-008 plus plan.md "Testing" require an unmodified stock-client conformance suite behind `cargo test -p spacestorage-conformance --features redis-dialect` (plan.md R14: complete-product profile only). Write failing tests first.

**Scope**: This feature widens `DialectProfile::CompleteProduct` Redis classification in `crates/compat` (the table `015` FR-016 assigns here), lowers in-table commands through the existing Redis handler onto the shared abstract type interface (`003`) and `QueryEngine` (`005`), and registers the one new L0 primitive `hyperloglog`. It adds **no** protocol, port, crate, binary, type hierarchy, or execution engine. The `016` first-binary string subset is unchanged.

**Path reconciliation** (plan.md "Project Type": no new crate/binary): plan.md's conceptual crate labels map to existing modules — plan `crates/l0` → `crates/types/src/l0.rs`; plan `crates/typeset` → `crates/types/src/catalog.rs` + `crates/types/src/descriptor.rs`; plan `crates/exec` → `crates/query/src/{request,planner,engine}.rs`. The `crates/handler-redis/src/commands/*.rs`, `lower.rs`, `render.rs`, `session.rs`, and `block.rs` files are new modules added by this feature.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: `[US1]`–`[US5]` on user-story phase tasks only
- Setup, Foundational, and Polish phases carry **no** story label
- Every task includes an exact file path

## Path Conventions

- Workspace root: `Cargo.toml`, `crates/`, `docs/`, `scripts/`
- Feature artifacts: `specs/017-redis-dialect-coverage/`
- Handler: `crates/handler-redis/src/`; matrix: `crates/compat/src/matrix.rs`; conformance: `crates/conformance/tests/`

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Workspace deps, handler module skeleton, and dialect config fixtures used by every story

- [X] T001 [P] Add the `redis` stock client as a dev-dependency and the `redis-dialect` feature (gated on `complete-product`, per research R14) to `crates/conformance/Cargo.toml`
- [X] T002 [P] Fold the existing `crates/handler-redis/src/commands.rs` into `crates/handler-redis/src/commands/mod.rs` (preserving its current `MUST_COMMANDS`/`RedisReply`/`SessionState`/`dispatch` exports — the crate cannot hold both `commands.rs` and `commands/mod.rs`), then create the new submodule files `crates/handler-redis/src/commands/{strings,hash,list,set,zset,stream,bitmap,hll,txn,pubsub,server,ss}.rs` plus `crates/handler-redis/src/{lower,render,session,block,expiry}.rs`, and declare them in `crates/handler-redis/src/commands/mod.rs` and `crates/handler-redis/src/lib.rs` (skeletons only; no behavior yet)
- [X] T003 [P] Register the dialect config fixtures in the config test suite: `specs/017-redis-dialect-coverage/contracts/fixtures/complete-product-redis.conf`, `.../memory-mode-pubsub.conf`, and `.../max-block-negative.conf` in `crates/config/tests/`

**Checkpoint**: Modules compile; the conformance crate exposes the `redis-dialect` feature.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Capability table, not-supported list, key-space entry model, shared reply/error rendering, session and blocking state, config knob, and profile plumbing. No user-story work can begin until this phase is complete.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [X] T004 Implement `CapabilityCommand { name: &'static str, group: ResourceGroup, profile: DialectProfile, options: &'static [&'static str] }` and `enum ResourceGroup { ConnectionMeta, KeyspaceIntrospection, StringExpiry, Hash, List, Set, SortedSet, Stream, Bitmap, PubSub, Transaction }` in `crates/compat/src/matrix.rs` and widen `classify_redis(profile, verb)` so `CompleteProduct` returns `Must` for every command in [contracts/capability-table.md](contracts/capability-table.md). Invariants (data-model §1): `FirstBinary` and `HandlersComplete` classify these commands `MustNot` (research R13); unknown verb → `MustNot`; an unknown option of a listed command → `MustNot`; `MustNot` never becomes a `LogicalRequest`
- [X] T005 Encode the closed not-supported list in `crates/compat/src/matrix.rs` from the canonical [contracts/not-supported.md](contracts/not-supported.md) (that file is the authoritative source of list membership) as always `MustNot` on every profile, matching that file's families exactly: geospatial (`GEOADD`, `GEODIST`, `GEOPOS`, `GEOHASH`, `GEORADIUS`, `GEORADIUSBYMEMBER`, `GEOSEARCH`, `GEOSEARCHSTORE`), `XAUTOCLAIM`, Lua/`EVAL`/`EVALSHA`/`SCRIPT`, `FUNCTION`/`FCALL`, `MODULE`, `CLUSTER`, `SENTINEL`, `ACL`, `MIGRATE`, `REPLICAOF`, `SLAVEOF`, `SYNC`, `PSYNC`, `DEBUG` (and its subcommands), `SHUTDOWN`, `RESTORE`, `DUMP`, `RENAME`, `RENAMENX`, `HELLO`/`HELLO 3`, `CLIENT SETINFO`, `SWAPDB`; option-level restrictions of otherwise-listed commands (for example `SELECT n>0`) stay with T004's unknown-option rule, not this command list; error form `-ERR unknown command '<COMMAND>'` (`crates/handler-redis/src/render.rs`)
- [X] T006 [P] Implement `RedisKeySpace { namespace: NamespaceName, container: ContainerRef, store_type: TypeName, mode: StorageMode }` and `RedisEntry { key: Bytes, visible: RedisVisibleType, payload: EntryPayload, expire_at: Option<Deadline>, version: u64 }` in `crates/handler-redis/src/lower.rs` (data-model §2/§3). Invariants: one container per namespace, default name `"redis"`, schema `"public"`, `store_type` L2 `kv_collection`; `visible` is a function of `payload` (`Bitmap`/`HyperLogLog` → `String`); first write sets both `visible` and `payload`; `expire_at` `None` = persistent; `version: u64` bumped on every mutation (WATCH reads it)
- [X] T007 [P] Implement RESP2 reply rendering for the new shapes and error forms in `crates/handler-redis/src/render.rs`: bulk/integer/nested-array replies, `-WRONGTYPE Operation against a key holding the wrong kind of value`, arity `-ERR wrong number of arguments for '<cmd>'`, limit `-ERR` naming the limit, `-EXECABORT`, and the RESP2 null array (`*-1`) per the data-model "Validation and errors" table and [contracts/redis-mappings.md](contracts/redis-mappings.md)
- [X] T008 [P] Implement assembled `RedisSession { namespace: NamespaceName, container: ContainerRef, authenticated: bool, txn: TransactionSession, subscriptions: Option<Subscription>, blocking: Option<BlockingWait>, options: QueryOptions }` in `crates/handler-redis/src/session.rs` (data-model §10); `subscriptions` `Some` ⇒ subscribe mode
- [X] T009 [P] Implement `Deadline { at: TtlInstant }` key/field expiry in `crates/handler-redis/src/expiry.rs` sharing the `013` TTL clock (`crates/storage/src/ttl.rs`): `at: TtlInstant` (event-time if present, else source-domain ingest/HLC); readers treat `at <= now` as missing (purge-on-read); `TTL`/`PTTL` report remaining or `-1` (no expiry) or `-2` (missing); `PERSIST` clears `expire_at` (data-model §7)
- [X] T010 [P] Implement `BlockingWait { connection: ConnectionId, keys: Vec<Bytes>, kind: BlockKind, deadline: Option<Instant> }` in `crates/handler-redis/src/block.rs` (data-model §9): at most one per connection; awaits a `tokio::sync::Notify` (never a worker-thread sleep, constitution II); dropped on reply, timeout, disconnect, or drain; `deadline` `None` only when `max_block` is unset **and** client timeout is 0
- [X] T011 [P] Implement `RedisProtocolConfig { max_block: Option<Duration> }` parse + validation in `crates/config/src/validate.rs` and `crates/config/src/model.rs` for `protocols.redis.max_block` (`None` = stock indefinite timeout 0); reject negative values with a named error (`crates/config/src/error.rs`)
- [X] T012 Implement the dispatch/lowering seam in `crates/handler-redis/src/commands/mod.rs` + `crates/handler-redis/src/lower.rs`: call `classify_outcome(profile, ProtocolId::Redis, verb)` **before** building a `LogicalRequest`; lower MUST commands to `TypeOp`/`LogicalRequest`; plumb the active `DialectProfile` from config (`CompleteProduct`) through `crates/handler-redis/src/lib.rs` and `crates/node/src/handler/redis.rs`
- [X] T013 [P] Add the multi-connection in-process conformance harness support (ephemeral port, complete-product profile, users file) in `crates/conformance/tests/support/mod.rs`, reusing the `002` harness
- [X] T014 [P] Add unit tests for `classify_redis` per profile and a full table walk of the not-supported list in `crates/compat/tests/redis_classify.rs` (each entry → `MustNot`; every capability-table command → `Must` on `CompleteProduct`, `MustNot` on `FirstBinary`/`HandlersComplete`)

**Checkpoint**: Foundation ready — table, key-space entry model, rendering, session/blocking state, config knob, and profile plumbing are in place. User stories can now begin.

---

## Phase 3: User Story 1 - Collection types on a stock Redis client (Priority: P1) 🎯 MVP

**Goal**: Hashes (incl. 7.4 hash-field expiry), lists, sets, sorted sets, and per-key expiry work against one namespace with an unmodified stock Redis client, keeping Redis reply shapes and error expectations (including `WRONGTYPE`).

**Independent Test**: For each documented family, run the documented command list with a stock client against one namespace and verify replies, error forms, and expiry behavior (`cargo test -p spacestorage-conformance --features redis-dialect -- collections`).

### Tests for User Story 1 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [X] T015 [P] [US1] Add hash-family conformance tests in `crates/conformance/tests/redis_collections.rs` for `HSET`/`HGET`/`HDEL`/`HGETALL`/`HLEN`/`HEXISTS`/`HINCRBY`/`HINCRBYFLOAT`/`HSTRLEN`/`HKEYS`/`HVALS`/`HMSET`/`HMGET`/`HSETNX`/`HRANDFIELD`/`HSCAN` and the hash-field expiry family (`HEXPIRE`, `HPEXPIRE`, `HEXPIREAT`, `HPEXPIREAT`, `HTTL`, `HPTTL`, `HEXPIRETIME`, `HPEXPIRETIME`, `HGETEX`, `HGETDEL`) per spec US1 scenario 1 (SC-001, SC-007 hash-field TTL)
- [X] T016 [P] [US1] Add list-family conformance tests in `crates/conformance/tests/redis_collections.rs` for `LPUSH`/`RPUSH`/`LPOP`/`RPOP`/`LRANGE`/`LINDEX`/`LLEN`/`LSET`/`LREM`/`LTRIM`/`LINSERT`/`LPUSHX`/`RPUSHX`/`RPOPLPUSH`/`LMOVE`/`BLPOP`/`BRPOP`/`BRPOPLPUSH`/`BLMOVE` (element order on both ends; blocking release on element arrival or named timeout) per spec US1 scenario 2
- [X] T017 [P] [US1] Add set-family conformance tests in `crates/conformance/tests/redis_collections.rs` for `SADD`/`SREM`/`SMEMBERS`/`SISMEMBER`/`SCARD`/`SPOP`/`SRANDMEMBER`/`SSCAN`/`SINTER`/`SUNION`/`SDIFF` and the `*STORE` forms per spec US1 scenario 3
- [X] T018 [P] [US1] Add sorted-set conformance tests in `crates/conformance/tests/redis_collections.rs` for `ZADD` (with `NX`/`XX`/`GT`/`LT`/`CH`/`INCR`), `ZRANGE` (incl. `BYSCORE`/`BYLEX`/`REV`), `ZRANGEBYSCORE`, `ZREVRANGE`, `ZREVRANGEBYSCORE`, `ZSCORE`, `ZMSCORE`, `ZCARD`, `ZCOUNT`, `ZINCRBY`, `ZRANK`, `ZREVRANK`, `ZREM`, `ZREMRANGEBYSCORE`, `ZREMRANGEBYRANK`, `ZRANGESTORE`, `ZPOPMIN`, `ZPOPMAX`, `BZPOPMIN`, `BZPOPMAX`, `ZUNION`/`ZINTER`/`ZDIFF` (no-destination forms), `ZUNIONSTORE`, `ZINTERSTORE`, `ZDIFFSTORE`, `ZSCAN` (score ordering, tie-break by member, inclusive/exclusive range syntax) per spec US1 scenario 4
- [X] T019 [P] [US1] Add key-expiry conformance tests in `crates/conformance/tests/redis_collections.rs` for `SETEX`/`PSETEX`/`SET … EX/PX`/`EXPIRE`/`PEXPIRE`/`EXPIREAT`/`PEXPIREAT`/`EXPIRETIME`/`PEXPIRETIME`/`TTL`/`PTTL`/`PERSIST` (key absent after expiry, reads-after behave missing, `TTL`/`PTTL` remaining lifetime) per spec US1 scenario 5 (SC-005)
- [X] T020 [P] [US1] Add a `WRONGTYPE` unit test in `crates/handler-redis/tests/wrongtype.rs`: a type-mismatched write (e.g. `LPUSH` on a `hash` key, `HGET` on a `string` key) replies `WRONGTYPE` naming the actual type and performs no partial operation (`version` unchanged) per spec US1 scenario 6 / FR-003
- [X] T021 [P] [US1] Add `TTL`/`PTTL` reporting unit tests (`-1` no expiry, `-2` missing) and hash-field expiry reporting (`HTTL`-family matching the set lifetime; field reads after expiry absent; key remains) in `crates/handler-redis/tests/expiry.rs`

### Implementation for User Story 1

- [X] T022 [P] [US1] Implement hash commands and payload types `HashValue { fields: Map<Bytes, HashField> }`, `HashField { value: Bytes, expire_at: Option<Deadline> }` (`None` = no field TTL) in `crates/handler-redis/src/commands/hash.rs`, lowering to `hash.*` `TypeOp`s (field-level semantics per FR-005)
- [X] T023 [P] [US1] Implement list commands and `ListValue { elements: Deque<Bytes> }` (`LPUSH`/`RPUSH` ends) in `crates/handler-redis/src/commands/list.rs`
- [X] T024 [P] [US1] Implement set commands and `SetValue { members: Set<Bytes> }` in `crates/handler-redis/src/commands/set.rs`
- [X] T025 [P] [US1] Implement sorted-set commands and `ZSetValue { by_member: Map<Bytes, f64>, by_score: Ordered<(f64, Bytes)> }` in `crates/handler-redis/src/commands/zset.rs`; score ordering is numeric with ties broken by member byte order; a `NaN` score is a command error and does not insert
- [X] T026 [US1] Implement string commands plus the key-expiry family in `crates/handler-redis/src/commands/strings.rs`: `GET`/`SET` (`NX`/`XX`/`EX`/`PX`/`KEEPTTL`)/`DEL`/`MGET`/`MSET`/`MSETNX`/`SETNX`/`SETEX`/`PSETEX`/`GETSET`/`STRLEN`/`APPEND`/`GETRANGE`/`SETRANGE`/`INCR`/`DECR`/`INCRBY`/`DECRBY`/`INCRBYFLOAT`/`GETDEL`/`GETEX`/`EXPIRE`/`PEXPIRE`/`EXPIREAT`/`PEXPIREAT`/`PERSIST`/`EXPIRETIME`/`PEXPIRETIME`; writes a `Deadline` on the entry (T009)
- [X] T027 [US1] Implement blocking list/zset forms `BLPOP`/`BRPOP`/`BRPOPLPUSH`/`BLMOVE`/`BZPOPMIN`/`BZPOPMAX` in `crates/handler-redis/src/commands/list.rs` + `crates/handler-redis/src/commands/zset.rs` using `BlockingWait` (T010) and the effective-timeout rule of FR-006 (timeout 0 indefinite unless `max_block` set, which overrides 0 and longer explicit timeouts)
- [X] T028 [US1] Enforce entry-type checking and no-partial-state transitions in `crates/handler-redis/src/lower.rs` + `crates/handler-redis/src/render.rs` (data-model "State transitions → Entry": same-type write bumps `version`; other-type command → `WRONGTYPE` unchanged; expire or `DEL` → absent)
- [X] T029 [US1] Lower collection and string commands to `TypeOp`/`LogicalRequest` on the shared interface in `crates/query/src/planner.rs` + `crates/query/src/request.rs`; the handler crate MUST NOT depend on `l0`/`storage` and MUST NOT hold a protocol-private store (constitution IV, `002` FR-035)
- [X] T030 [US1] Auto-create the namespace key-space container on first write and inherit its `mode` as the durability tier of every entry in `crates/handler-redis/src/lower.rs` + `crates/handler-redis/src/commands/mod.rs` (data-model §2 invariant; `002` auto-create)

**Checkpoint**: All documented collection and expiry families pass the stock-client suite; `WRONGTYPE` and no-partial-state verified. `cargo test -p spacestorage-conformance --features redis-dialect` green for US1.

---

## Phase 4: User Story 2 - Streams with consumer groups (Priority: P1)

**Goal**: Producers `XADD` entries; a group is created with `XGROUP`; consumers read with `XREAD`/`XREADGROUP`, acknowledge with `XACK`, inspect with `XPENDING`, and reclaim with `XCLAIM`; trimming/inspection behave as expected; entries ordered by ID and survive the storage tier of the key's entry.

**Independent Test**: With a stock client, produce entries and consume them through a named consumer group with explicit ack and reclaim; verify ID ordering, group offsets, and pending lists (`cargo test -p spacestorage-conformance --features redis-dialect -- streams`).

### Tests for User Story 2 ⚠️

- [X] T031 [P] [US2] Add stream conformance tests in `crates/conformance/tests/redis_streams.rs` for `XADD` (`*` and explicit IDs), `XLEN`, `XRANGE`/`XREVRANGE`/`XREAD` (with and without `BLOCK` and count), `XGROUP CREATE`/`DESTROY`/`SETID`/`DELCONSUMER`/`CREATECONSUMER`/`HELP`, `XREADGROUP`, `XACK`, `XPENDING` (summary + extended), `XCLAIM`, `XDEL`, `XTRIM` (`MAXLEN`/`MINID`/`~`), and `XINFO STREAM`/`GROUPS`/`CONSUMERS` per spec US2 scenarios 1–6 (SC-003)
- [X] T032 [P] [US2] Add a restart durability conformance test in `crates/conformance/tests/redis_streams.rs`: a persistent/hybrid stream restores content, last-delivered positions, and group offsets across a boot of the same data dir; memory-mode does not (FR-016)
- [X] T033 [P] [US2] Add stream-ID unit tests in `crates/handler-redis/tests/stream_ids.rs`: `*` IDs strictly increase; `0-*` allocates the next sequence; an explicit ID ≤ `last_id` is rejected and appends nothing; a stale total-order lease epoch is refused and is not an entry (data-model §4 invariants)

### Implementation for User Story 2

- [X] T034 [P] [US2] Implement `StreamId { ms: u64, seq: u64 }`, `StreamEntry { id: StreamId, fields: Vec<(Bytes, Bytes)> }`, `enum StreamOrder { Total, Producer }` (default `Total`), and `StreamValue { order, entries: OrderedMap<StreamId, StreamEntry>, last_id, groups: Map<Bytes, ConsumerGroup>, lease: Option<LeaseEpoch> }` in `crates/handler-redis/src/commands/stream.rs` (data-model §4); `lease` is `Some` iff `order == Total`
- [X] T035 [P] [US2] Implement `ConsumerGroup { name: Bytes, last_delivered: StreamId, pending: Map<StreamId, PendingEntry> }` and `PendingEntry { id, consumer: Bytes, delivery_count: u64, delivered_at: Timestamp, body: StreamEntry }` (idle time = now − `delivered_at`) plus `XGROUP`/`XACK`/`XPENDING`/`XCLAIM` semantics in `crates/handler-redis/src/commands/stream.rs`
- [X] T036 [US2] Implement `XADD`/`XREAD`/`XREADGROUP` (incl. `BLOCK`) in `crates/handler-redis/src/commands/stream.rs`; at-most-one-consumer-at-a-time delivery; each consumer keeps its own position; `XREAD BLOCK` reuses `BlockingWait` (T010)
- [X] T037 [US2] Implement `XTRIM`/`XLEN`/`XRANGE`/`XREVRANGE`/`XDEL`/`XINFO` in `crates/handler-redis/src/commands/stream.rs`; trim drops oldest IDs only (surviving entries keep id + body) and removes PEL entries for deleted IDs per Redis 7.4 trim rules
- [X] T038 [US2] Integrate the total-order leadership lease: request a per-stream-key `LeadershipLease` epoch from `crates/controlplane/src/lease.rs`; appends present the current epoch; a stale epoch returns an unavailable error (`-ERR` naming the command) and stores nothing; with no lease holder the command writes nothing and does not fall open into producer order (research R6, constitution XII)
- [X] T039 [US2] Implement producer-order selection at key creation (the documented `XADD` creation-time option surfaced per [contracts/redis-mappings.md](contracts/redis-mappings.md) intent / plan.md R6) in `crates/handler-redis/src/commands/stream.rs`; producer-order streams take no lease
- [X] T040 [US2] Persist stream content, `last_id`, `groups.last_delivered`, and `pending` inside the container value through `013`'s WAL/restore (`crates/storage/src/content.rs`, `crates/storage/src/restore.rs`) with memory mode unpromised (data-model §4 invariant)
- [X] T041 [US2] Reject `XAUTOCLAIM` before the stream struct is touched (always `MustNot`) in `crates/compat/src/matrix.rs` + `crates/handler-redis/src/commands/stream.rs`; reply `-ERR unknown command 'XAUTOCLAIM'`

**Checkpoint**: US1 and US2 both work independently; group offsets survive a persistent-stream restart.

---

## Phase 5: User Story 3 - Transactions and pub/sub (Priority: P2)

**Goal**: `MULTI`/`EXEC` with Redis queued-and-atomic no-rollback semantics; `WATCH`/`UNWATCH` optimistic locking aborting `EXEC`; namespace-scoped `SUBSCRIBE`/`PSUBSCRIBE`/`PUBLISH` delivering messages including the publisher's own connection.

**Independent Test**: Run the documented transaction and pub/sub workflows with two or more stock clients over separate connections and verify queueing, atomicity, watch abort, and message delivery (`cargo test -p spacestorage-conformance --features redis-dialect -- transactions` and `-- pubsub`).

### Tests for User Story 3 ⚠️

- [X] T042 [P] [US3] Add transaction + pub/sub conformance tests in `crates/conformance/tests/redis_txn_pubsub.rs`: `MULTI`/`EXEC` order and multi-bulk reply, `DISCARD`, inline `EXEC` error with later commands still running and earlier writes applied (no rollback), `WATCH` abort on a concurrent modification (null reply), `PSUBSCRIBE` pattern delivery with original + matched pattern, the sharded forms `SPUBLISH`/`SSUBSCRIBE`/`SUNSUBSCRIBE` sharing the single-node namespace-scoped bus, `UNSUBSCRIBE`-all leaving command mode, and namespace isolation (a publish in namespace A is invisible in namespace B) per spec US3 scenarios 1–5 (SC-004)
- [X] T043 [P] [US3] Add transaction unit tests in `crates/handler-redis/tests/txn.rs`: a queuing-time arity/syntax error flags `dirty`; `EXEC` on `dirty` returns `-EXECABORT` and runs nothing; blocking commands, `SUBSCRIBE`, and nested `MULTI` in the queue are queuing errors; `WATCH` outside `MULTI` is held until `EXEC`/`DISCARD`/`UNWATCH`/disconnect (data-model §6 invariants)

### Implementation for User Story 3

- [X] T044 [P] [US3] Implement `TransactionSession { state: TxnState { Idle, Queuing { dirty: bool }, Executing }, queue: Vec<QueuedCommand>, watched: Map<Bytes, u64> }` and `QueuedCommand { argv: Vec<Bytes>, lowered: LogicalRequest }` in `crates/handler-redis/src/commands/txn.rs`; `watched` maps key → `RedisEntry.version` at `WATCH` time (data-model §6)
- [X] T045 [US3] Implement `MULTI`/`EXEC`/`DISCARD`/`WATCH`/`UNWATCH` semantics in `crates/handler-redis/src/commands/txn.rs`: `EXEC` runs the queue in order as one `005` session batch under default `READ COMMITTED` (never `SERIALIZABLE`); an execution error is one reply element inline; a watched-key missing/version-changed aborts with the RESP2 null array and discards the queue
- [X] T046 [P] [US3] Implement `Subscription { connection: ConnectionId, namespace: NamespaceId, channels: Set<Bytes>, patterns: Set<Bytes> }` and `PubSubMessage { namespace, channel, payload }` in `crates/handler-redis/src/session.rs` plus an in-process `PubSubBus` keyed by namespace id in `crates/node/src/handler/live_map.rs` (or a sibling module); delivery only when `message.namespace == subscription.namespace`; never a WAL record (data-model §5)
- [X] T047 [US3] Implement `SUBSCRIBE`/`UNSUBSCRIBE`/`PSUBSCRIBE`/`PUNSUBSCRIBE`/`PUBLISH`/`PUBSUB`/`SPUBLISH`/`SSUBSCRIBE`/`SUNSUBSCRIBE` and subscribe-mode reply shapes (incl. `PING` array form) in `crates/handler-redis/src/commands/pubsub.rs`; `PUBLISH` returns the matching-subscription count on **this process**
- [X] T048 [US3] Enforce namespace scoping and non-durability in `crates/handler-redis/src/commands/pubsub.rs` + `crates/handler-redis/src/session.rs`: no message/channel/pattern crosses a namespace; restart drops every `Subscription`; `UNSUBSCRIBE`-all and `PUNSUBSCRIBE`-all return to command mode

**Checkpoint**: Transactions and pub/sub work independently of US1/US2 storage and never cross namespace boundaries.

---

## Phase 6: User Story 4 - INFO and introspection (Priority: P2)

**Goal**: `INFO` (bare and per-section) replies in Redis CRLF format with the documented sections expressing this node's and namespace's real state; companion `DBSIZE`, documented `SCAN`/`OBJECT` subset, `TYPE`, and the `COMMAND` summary line are present.

**Independent Test**: With a stock client, call `INFO` with no argument and with each documented section; verify format, section coverage, and that counters move after known data operations (`cargo test -p spacestorage-conformance --features redis-dialect -- info`).

### Tests for User Story 4 ⚠️

- [X] T049 [P] [US4] Add INFO/introspection conformance tests in `crates/conformance/tests/redis_info.rs`: bare `INFO` and each documented section (`server`, `clients`, `memory`, `stats`, `keyspace`, `replication`) with `# section` header lines and CRLF `field:value`; `INFO keyspace`/`DBSIZE` counts match live non-expired keys; `INFO stats` counters increase after known writes/reads/expired deletions per spec US4 scenarios 1–3 (SC-006); the R1 connection/server-meta commands `PING`, `ECHO`, `TIME`, `QUIT`, `SELECT 0`, the documented `CLIENT` subset (`ID`/`INFO`/`LIST`/`KILL`/`SETNAME`/`GETNAME`/`PAUSE`), `CONFIG GET`/`CONFIG SET` for `max_block`, and `COMMAND`/`COMMAND INFO`/`COMMAND COUNT` listing the capability table all return their documented reply shapes
- [X] T050 [P] [US4] Add `TYPE`/`OBJECT` unit tests in `crates/handler-redis/tests/introspection.rs`: `TYPE` returns the Redis-visible name (`string`, `hash`, `list`, `set`, `zset`, `stream`) matching the creating command; a bitmap and HLL key report `string`; `OBJECT ENCODING`/`IDLETIME` are in the subset and other `OBJECT` subcommands are not-supported per spec US4 scenario 4 (FR-004)

### Implementation for User Story 4

- [X] T051 [P] [US4] Implement the `INFO` renderer in `crates/handler-redis/src/commands/server.rs`: CRLF `# section` blocks for `server`/`clients`/`memory`/`stats`/`keyspace`/`replication` (+ `commandstats` when counters exist); values read from `008`/node stats (`crates/node/src/stats.rs`); `redis_version = 7.4.0-spacestorage`; `replication` reports this node's real role (never a hardcoded `role:master`); fields with no internal equivalent (`mem_fragmentation_ratio`, `aof_enabled`, …) are omitted, never constant fakes (FR-010, research R10, constitution X)
- [X] T052 [P] [US4] Implement `DBSIZE`, `TYPE`, `SCAN`, `RANDOMKEY`, `TOUCH`, `UNLINK`, and the documented `OBJECT` subset (`ENCODING`, `IDLETIME`; other subcommands not-supported) in `crates/handler-redis/src/commands/server.rs`
- [X] T053 [P] [US4] Implement `COMMAND`/`COMMAND INFO`/`COMMAND COUNT` listing the capability table in `crates/handler-redis/src/commands/server.rs` so the dialect is discoverable without trial-and-error (FR-011)
- [X] T054 [US4] Implement `TIME`, `ECHO`, `QUIT`, the `CLIENT` subset (`ID`/`INFO`/`LIST`/`KILL` by ID/`SETNAME`/`GETNAME`/`PAUSE`), `CONFIG GET`/`CONFIG SET` (only `max_block`; other keys omitted or an error), and reject `SWAPDB` in `crates/handler-redis/src/commands/server.rs`
- [X] T055 [US4] Map `INFO keyspace`/`DBSIZE` to the namespace's live non-expired key-space entries and wire `INFO stats` counters to the product's existing `008` series (existing `protocol`/`namespace` labels; no renamed or dropped required series) in `crates/handler-redis/src/commands/server.rs` + `crates/node/src/stats.rs`

**Checkpoint**: `INFO` and companion introspection are independently testable and reflect real node/namespace state.

---

## Phase 7: User Story 5 - Bitmaps and HyperLogLog (Priority: P3)

**Goal**: Bitmaps (`SETBIT`, `GETBIT`, `BITCOUNT`, `BITPOS`, `BITOP`) riding the existing L0 `bitmap`; HyperLogLog (`PFADD`, `PFCOUNT`, `PFMERGE`) backed by a registered L0 `hyperloglog` cardinality-sketch primitive, reachable only through the shared abstract type interface.

**Independent Test**: For each family, run the documented commands with a stock client and verify reply values within the documented accuracy limits (`cargo test -p spacestorage-conformance --features redis-dialect -- bitmap`).

### Tests for User Story 5 ⚠️

- [X] T056 [P] [US5] Add bitmap conformance tests in `crates/conformance/tests/redis_bitmap_hll.rs`: `SETBIT` at scattered offsets, `GETBIT`, `BITCOUNT` with range syntax, `BITPOS`, and `BITOP` (`AND`/`OR`/`XOR`/`NOT` to a destination key) with missing-bit default 0 and the `BITOP` length rule, plus `BITFIELD`/`BITFIELD_RO` signed/unsigned get/set with `WRAP`/`SAT`/`FAIL` overflow modes; `TYPE` reports `string` per spec US5 scenario 1
- [X] T057 [P] [US5] Add HyperLogLog conformance tests in `crates/conformance/tests/redis_bitmap_hll.rs`: `PFADD`/`PFCOUNT`/`PFMERGE` with a fixed distinct-element fixture landing inside `1.04/√m` at precision 14 (asserted as a bound, not an exact integer), merged sketches behaving like a sketch of the union, and `TYPE` reporting `string` per spec US5 scenario 2 (SC-007)
- [X] T058 [P] [US5] Add the type-catalog test in `crates/types/tests/hyperloglog_extension.rs`: the `hyperloglog` cardinality-sketch type appears at level L0 with its accuracy bound, canonical representation, storage modes, and operation set; the same `PFADD`/`PFCOUNT`/`PFMERGE` behavior is reachable through the shared abstract type interface; baseline type count stays 55 per spec US5 scenario 3 (FR-017, `003` FR-040/FR-046)

### Implementation for User Story 5

- [X] T059 [P] [US5] Implement the L0 `hyperloglog` primitive in `crates/types/src/l0.rs` (plan's `crates/l0`): add `"hyperloglog"` to `L0_CREATABLE`, add its `display_for` label and an `L0Store` variant; `HllValue { precision: u8, registers }` with `precision` frozen at `PFADD` create; `add`/`count`/`merge`/`clear` operations
- [X] T060 [P] [US5] Register `HyperLogLogDescriptor { name: "hyperloglog", level: L0, kind: DataStructure, creatable: true, modes: { memory: default, persistent, hybrid }, schema: Optional { precision: u8 }, // default 14, range 4..=16, operations: ["add","count","merge","clear"], standard_error: "1.04/sqrt(2^precision)", canonical_version: 1 }` in `crates/types/src/catalog.rs` + `crates/types/src/descriptor.rs` (plan's `crates/typeset`) through the `003` FR-045–FR-049 extension path; baseline descriptors and the count 55 are unchanged (`003` FR-046)
- [X] T061 [P] [US5] Implement bitmap commands riding the existing L0 `bitmap` (`crates/types/src/l0.rs`) in `crates/handler-redis/src/commands/bitmap.rs`: `SETBIT`/`GETBIT`/`BITCOUNT`/`BITPOS`/`BITOP` (`AND`/`OR`/`XOR`/`NOT` writing a destination key), with `TYPE` reporting `string`; an offset past the `015` `limits.max_value` rejects as a limit error before the op; `BITFIELD`/`BITFIELD_RO` with `WRAP`/`SAT`/`FAIL` overflow modes
- [X] T062 [US5] Implement `PFADD`→`add`, `PFCOUNT`→`count` (sparse inputs merge for the estimate), and `PFMERGE`→`merge`+`clear` in `crates/handler-redis/src/commands/hll.rs`, creating the destination at the maximum precision of its inputs when missing; a `PFADD` against a key whose visible type is not `string` or whose payload is not `HyperLogLog` is `WRONGTYPE`; the handler reaches the type only through the shared abstract interface (no protocol-private encoding)

**Checkpoint**: All user stories are independently functional; bitmap and HLL behavior verified within documented bounds.

---

## Phase 8: Polish & Cross-Cutting Concerns

**Purpose**: Suite-wide contracts, limits, cross-protocol visibility, config validation, first-binary regression, and quickstart

- [X] T063 [P] Add the not-supported conformance suite `crates/conformance/tests/redis_not_supported.rs`: every family in [contracts/not-supported.md](contracts/not-supported.md) returns `-ERR unknown command '<COMMAND>'` and never a `+OK`/empty-array success (`CLUSTER INFO`/`NODES` may reply "disabled" info) across SC-002
- [X] T064 [P] Add the limits conformance suite `crates/conformance/tests/redis_limits.rs`: exceeding a documented key/value size, admission, or quota limit through a Redis command rejects with the limit named in Redis form in 100% of cases; out-of-range bitmap offsets, absurd counts/limits, and stream ID-boundary overflow return documented limit errors, with 0 hangs (SC-008)
- [X] T065 [P] Add the cross-protocol shared-object test in `crates/conformance/tests/redis_not_supported.rs` (or a sibling): a string written via Redis `SET` is read back through the canonical `SS.GET`/PostgreSQL path on the same container and the entry type is visible (FR-003 / `002` shared-object promise, research R14 integration note)
- [X] T066 [P] Add config validation tests in `crates/config/tests/redis_dialect.rs`: `max-block-negative.conf` fails with the named `max_block` error; `complete-product-redis.conf` and `memory-mode-pubsub.conf` parse, and `max_block` unset resolves to stock indefinite timeout 0 (`crates/config/src/validate.rs`)
- [X] T067 Add first-binary regression tests in `crates/compat/tests/redis_classify.rs` + `crates/conformance/tests/dialect_redis.rs`: `HGET`, `XADD`, `MULTI`, `SUBSCRIBE`, `PFADD`, `SETBIT`, `INFO`, `COMMAND`, `TYPE`, and `DBSIZE` still error on `DialectProfile::FirstBinary` and `HandlersComplete` (no new MUST verbs; `016` SC-006 / FR-005 preserved, research R13)
- [X] T068 [P] Validate [quickstart.md](quickstart.md) end-to-end with a stock `redis-cli`/client against a `CompleteProduct` node (Steps 1–11 plus the first-binary negative check) and record any mismatch as a follow-up
  - Follow-up (T068): the executable mirror `crates/conformance/tests/redis_quickstart.rs` is green (12/12, Steps 1–11 + first-binary negative). Validation itself completed; the two `quickstart.md` defects it surfaced (not product defects) are now **fixed in the doc**: (a) the Step 3 `HGETALL` line notes that field order is unspecified (the handler emits fields lexicographically); (b) Step 4 creates the group at `0` instead of `$`, so `XREADGROUP ... >` returns the existing pending entry (with `$`, stock Redis returns nil too).
- [X] T069 [P] Document the dialect surface and the single new knob in `docs/redis-dialect.md`: the capability table, the not-supported list, the Redis↔store type mapping, and `protocols.redis.max_block` default "unset = stock timeout 0" (per constitution XI)
- [X] T070 [P] Add the full-capability-table conformance walk `crates/conformance/tests/redis_capability_walk.rs`: iterate every command row in [contracts/capability-table.md](contracts/capability-table.md) and assert a stock-client round-trip per row (0 failures), reusing the per-family suite helpers where a row is already covered, so SC-001's "100% of capability-table commands pass" is directly demonstrable

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately
- **Foundational (Phase 2)**: Depends on Setup — BLOCKS all user stories
- **User Stories (Phases 3–7)**: All depend on Foundational completion; proceed in priority order (P1 → P1 → P2 → P2 → P3) or in parallel when staffed
- **Polish (Phase 8)**: Depends on all desired user stories being complete

### User Story Dependencies

- **US1 (P1)**: Starts after Foundational — no dependency on other stories
- **US2 (P1)**: Starts after Foundational — uses the shared `Deadline`/`BlockingWait`/key-space model from Phase 2, otherwise independent of US1
- **US3 (P2)**: Starts after Foundational — transactions lower queued commands through the same `lower.rs` seam; pub/sub is connection-scoped and storage-independent
- **US4 (P2)**: Starts after Foundational — reads the key-space counters, so it integrates with US1's container but is independently testable
- **US5 (P3)**: Starts after Foundational — bitmap/HLL ride existing/registered L0 primitives; independent of US1–US4

### Within Each User Story

- Tests are written and MUST FAIL before implementation
- Models/payload types before command services
- Command services before blocking/integration wiring
- Core implementation before cross-story integration
- Story complete before moving to the next priority (or run in parallel)

### Parallel Opportunities

- All Setup tasks marked `[P]` run in parallel
- Foundational `[P]` tasks (T006, T007, T008, T009, T010, T011, T013, T014) run in parallel; T004 → T005 → T012 are sequential (same `matrix.rs`/dispatch seam)
- Once Foundational completes, all user stories can start in parallel
- Within a story, all test tasks and all `[P]` model tasks run in parallel
- Polish `[P]` tasks run in parallel

---

## Parallel Example: User Story 1

```bash
# Launch all US1 tests together:
Task: "Hash-family conformance tests in crates/conformance/tests/redis_collections.rs"
Task: "List-family conformance tests in crates/conformance/tests/redis_collections.rs"
Task: "Set-family conformance tests in crates/conformance/tests/redis_collections.rs"
Task: "Sorted-set conformance tests in crates/conformance/tests/redis_collections.rs"
Task: "Key-expiry conformance tests in crates/conformance/tests/redis_collections.rs"

# Launch the payload/command modules together:
Task: "Implement hash commands in crates/handler-redis/src/commands/hash.rs"
Task: "Implement list commands in crates/handler-redis/src/commands/list.rs"
Task: "Implement set commands in crates/handler-redis/src/commands/set.rs"
Task: "Implement zset commands in crates/handler-redis/src/commands/zset.rs"
```

## Parallel Example: User Story 2

```bash
Task: "Stream + consumer-group implementation in crates/handler-redis/src/commands/stream.rs"
Task: "ConsumerGroup/PendingEntry + XGROUP/XACK/XPENDING/XCLAIM in crates/handler-redis/src/commands/stream.rs"
```

> Note: T034, T035, T036, T037 all touch `crates/handler-redis/src/commands/stream.rs`, so only the test tasks (T031–T033) run truly in parallel; sequence the `stream.rs` edits.

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL — blocks all stories)
3. Complete Phase 3: User Story 1
4. **STOP and VALIDATE**: `cargo test -p spacestorage-conformance --features redis-dialect -- collections` plus the `WRONGTYPE` and expiry unit tests
5. Deploy/demo the everyday collection types on a stock client

### Incremental Delivery

1. Setup + Foundational → the capability table, key-space entry model, and rendering are ready
2. US1 → validate independently → deliver (MVP)
3. US2 → validate independently → deliver (Streams + groups)
4. US3 → validate independently → deliver (transactions + pub/sub)
5. US4 → validate independently → deliver (INFO + introspection)
6. US5 → validate independently → deliver (bitmaps + HyperLogLog)
7. Polish → suite-wide not-supported/limit/cross-protocol/config checks and quickstart

### Parallel Team Strategy

1. Team completes Setup + Foundational together
2. Then: Developer A → US1, Developer B → US2, Developer C → US3, and later US4/US5
3. Each story integrates without breaking the others; the first-binary profile stays green throughout

---

## Notes

- `[P]` = different files, no dependencies on incomplete tasks
- `[Story]` maps each task to a user story for traceability
- Tests are stock-client workflows under the `redis-dialect` complete-product feature only; first-binary CI gains no new MUST verbs
- Every task has an exact file path; paths use the real modules per the Path reconciliation note
- Constitution gates: one Rust/Tokio stack, no protocol-private type system, fsync stays on the `013` blocking pool, total-order stream writes request a `006` lease, pub/sub is in-process and not durable
