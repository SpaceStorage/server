# Feature Specification: Redis Dialect Coverage

**Feature Branch**: `017-redis-dialect-coverage`

**Created**: 2026-10-31

**Status**: Draft

**Input**: User description: "capability of redis: INFO, Streams: XADD, XREAD, XGROUP and so on, lists, sets, sorted sets, hashes (HGET/HSET…), geo, bitmaps, HyperLogLog, pub/sub, transactions, setex"

## Clarifications

### Session 2026-10-31

- Q: Should a blocking Redis command be allowed to block indefinitely when the client passes no timeout (`BLPOP` key … 0), or should the server always cap it with a configurable maximum block time? → A: Indefinite blocking is honored (stock semantics: timeout 0 means block indefinitely), with an administrator-configurable maximum block time that overrides timeout 0 and longer explicit timeouts. Default deployments keep stock behavior while operators can bound parked connections for drain and capacity management.
- Q: What ordering guarantee must one stream key give when several producers write to it at the same time? → A: Per-key configurable at stream-key creation (revised from an initial total-order answer): (1) total order — entries from all producers interleave into one ID sequence every observer agrees on, writes serialized through a single writer whose designation is requested from the control plane (constitution `XII`); or (2) producer order — each producer's writes keep their own order and different producers' entries interleave arbitrarily. The default model, including implicit creation by `XADD`, is total order, which stock-Redis clients rely on; group offsets are defined against the chosen model.
- Q: When a Redis client writes a key that does not exist yet with a collection command — for example `HSET`, `LPUSH`, `ZADD`, or `XADD` on a missing key — how does that key become a container in the database? → A: Single default container. Each namespace has one default Redis key-space container; every Redis key is a typed entry inside it (type per entry: `string`, `hash`, `list`, `set`, `zset`, `stream`), created implicitly by the first write command that targets the key. The key-space container exists as one shared store object; a write of a different type against an existing key returns `WRONGTYPE`.
- Q: Which Redis version's command semantics should the capability table pin as its baseline? → A: Redis 7.4-line semantics (hash-field expiry family in scope), with two explicit user exclusions below that baseline: the geospatial command family (`GEOADD`, `GEODIST`, `GEOPOS`, `GEOHASH`, `GEORADIUS`, `GEOSEARCH`) and `XAUTOCLAIM` are NOT in scope and MUST return the not-supported error form.

### Session 2026-11-26

- Q: Reviewing the dialect by resource group, which groups stay in scope for this feature? → A: Groups 1–9 of the resource review stay in scope and are owned by this feature: (1) connection/server meta, (2) keyspace introspection, (3) string + expiry, (4) hash, (5) list, (6) set, (7) sorted set, (8) streams + consumer groups, (9) bitmaps. Connection-scoped pub/sub and transactions (groups 11–12 of that review) remain as the separate story already in this spec; the geospatial family and `XAUTOCLAIM`, Lua/`EVAL`/functions, modules, `CLUSTER`, Sentinel roles, and ACL-management commands stay out of scope and MUST return the not-supported error form.
- Q: HyperLogLog is a P3 family of the capability table; does its state live in a protocol-private string encoding or in a registered store type? → A: It MUST be backed by a **first-class foundation primitive type** — a cardinality sketch at level L0, alongside `Bloom Filter` — registered through the type system's extension path (`03` FR-045–FR-049) and reachable only through the shared abstract type interface; not a protocol-private encoding. `PFADD`/`PFCOUNT`/`PFMERGE` map onto its operations, its descriptor carries the standard-error bound of FR-005 and a canonical self-describing representation, and this feature registers the type in the `03` L0 inventory.

## User Scenarios & Testing *(mandatory)*

The first-binary Redis dialect (intent `16`) is a narrow string key/value
subset. This feature defines the **complete-product Redis dialect**: a stock
Redis client connects and works with the Redis data types it expects —
hashes, lists, sets, sorted sets, Streams, pub/sub, transactions, expiry,
bitmaps, HyperLogLog — and introspects the server with `INFO`,
without learning that it is not talking to a stock Redis server. The dialect
is a **capability table** owned by this feature: Redis commands in the table
MUST work, commands explicitly outside the table MUST return the Redis
not-supported error form, never a silent empty success.

### User Story 1 — Collection types on a stock Redis client (Priority: P1)

A developer points an existing Redis application client at the server and
works with the everyday collection types: hashes (`HSET`/`HGET`/`HGETALL`…),
lists (`LPUSH`/`RPOP`/`LRANGE`…), sets (`SADD`/`SINTER`/`SMEMBERS`…), and
sorted sets (`ZADD`/`ZRANGE`/`ZSCORE`…). Expiry works per key with the
`SETEX`/`PEXPIRE`/`TTL` family (`setex`). The client keeps its driver, its
reply shapes, and its error expectations (including `WRONGTYPE`).

**Why this priority**: collection types plus expiry are the bulk of real
Redis application workloads; without them the Redis dialect stays a toy
key/value bridge.

**Independent Test**: For each documented family, run the documented list of
commands with an unmodified stock Redis client against one namespace and
verify replies, error forms, and expiry behavior.

**Acceptance Scenarios**:

1. **Given** a connected authenticated Redis session, **When** the client
   writes and reads a field of a hash (`HSET`, `HGET`, `HDEL`, `HGETALL`,
   `HLEN`, `HEXISTS`, `HINCRBY`, `HKEYS`, `HVALS`, per-key TTL and the
   hash-field expiry family), **Then** all return the Redis reply shapes
   and the field values round-trip unchanged.
2. **Given** an existing list, **When** the client pushes, pops, ranges,
   indexes, trims, and blocks on it (`LPUSH`, `RPUSH`, `LPOP`, `RPOP`,
   `LRANGE`, `LINDEX`, `LLEN`, `LSET`, `LREM`, `LTRIM`, `LINSERT`, `BLPOP`,
   `BRPOP`), **Then** element order matches Redis semantics on both ends
   and blocking commands release on element arrival or named timeout.
3. **Given** members added to a set, **When** the client uses `SADD`,
   `SREM`, `SMEMBERS`, `SISMEMBER`, `SCARD`, `SPOP`, `SRANDMEMBER`,
   `SINTER`, `SUNION`, `SDIFF`, and one member of each `*STORE` form, **Then**
   membership and the three set operations match Redis semantics.
4. **Given** members with scores in a sorted set, **When** the client uses
   `ZADD` (with `NX`/`XX`/`GT`/`LT`/`CH`/`INCR`), `ZRANGE` (including
   `BYSCORE`/`BYLEX`/`REV`), `ZRANGEBYSCORE`, `ZREVRANGE`, `ZSCORE`,
   `ZCARD`, `ZCOUNT`, `ZINCRBY`, `ZRANK`, `ZREVRANK`, `ZREM`,
   `ZREMRANGEBYSCORE`, `ZPOPMIN`, `ZPOPMAX`, and `BZPOPMIN`, **Then** score
   ordering, tie-breaks by member, and inclusive/exclusive range syntax
   match Redis semantics.
5. **Given** a key with a lifetime, **When** the client uses `SETEX`,
   `PSETEX`, `SET … EX/PX`, `EXPIRE`, `PEXPIRE`, `EXPIREAT`, `PEXPIREAT`,
   `TTL`, `PTTL`, and `PERSIST`, **Then** the key expires at the requested
   time, reads before expiry succeed, reads after expiry behave as
   missing, and `TTL`/`PTTL` report remaining lifetime.
6. **Given** a string-typed key holding a hash and a client issuing
   `LPUSH` on it, **When** the command executes, **Then** the server
   replies with the `WRONGTYPE` error naming the actual type and performs
   no partial operation.

---

### User Story 2 — Streams with consumer groups (Priority: P1)

A service uses Streams as its event log: a producer `XADD`s entries, a
consumer group is created with `XGROUP`, consumers read with `XREAD` and
`XREADGROUP`, acknowledge with `XACK`, inspect pending entries with
`XPENDING`, and reclaim stalled entries with `XCLAIM`. Trimming and
inspection (`XLEN`, `XRANGE`, `XREVRANGE`, `XDEL`, `XTRIM`, `XINFO`)
behave as expected. Entries are ordered by their ID and survive the
durability tier of their stream key's entry.

**Why this priority**: Streams carry messaging and event-sourcing workloads;
this is the item from the previous not-supported list with the highest
demand, and it drives the shared storage model out of key/value only.

**Independent Test**: With an unmodified stock Redis client, produce
entries and consume them through a named consumer group with explicit
acknowledgement and reclaim, then verify ID ordering, group offsets, and
pending lists.

**Acceptance Scenarios**:

1. **Given** a stream, **When** the client `XADD`s entries with `*` and
   with explicit IDs, **Then** automatically generated IDs strictly
   increase, time-derived IDs follow Redis ordering rules, explicit IDs
   below the last ID are rejected, and `XLEN` reports the count.
2. **Given** entries in the stream, **When** the client reads with
   `XRANGE`/`XREVRANGE`/`XREAD` (with and without `BLOCK` and count), **Then**
   entries return in ID order with field-name/field-value pairs preserved,
   and a blocked `XREAD` returns on the next entry or times out.
3. **Given** a group created with `XGROUP CREATE` at `$` or a chosen ID,
   **When** a consumer reads with `XREAD GROUP`, **Then** each entry is
   delivered to exactly one consumer at a time, each consumer keeps its
   own position, and delivery history comes back as the Redis reply shape.
4. **Given** delivered entries, **When** the consumer `XACK`s them, **Then**
   they leave the group's pending list; **When** `XPENDING` is queried
   (summary and extended forms) after deliveries, **Then** unacknowledged
   entries are listed with consumer, idle time, and delivery count.
5. **Given** a stalled consumer's pending entry, **When** another consumer
   uses `XCLAIM` with the documented idle time and force/reset options,
   **Then** the entry moves to the claiming consumer and keeps its
   delivery count, incremented. (`XAUTOCLAIM` is a user exclusion: it
   returns the not-supported error form naming the command.)
6. **Given** a stream of max length N (`XTRIM MAXLEN` or `MAXLEN` on
   `XADD`, or approximate `~` trimming), **When** trimming runs, **Then**
   the oldest IDs beyond N are removed and entries still present keep
   their IDs and content.

---

### User Story 3 — Transactions and pub/sub (Priority: P2)

A client that needs atomic multi-command updates uses `MULTI`/`EXEC` with
Redis semantics: commands are queued, execution is atomic, and an error in
one queued command does not roll back the others that ran. A client that
needs cross-key optimistic locking uses `WATCH`/`UNWATCH` to abort `EXEC`
when a watched key changes meanwhile. A different client pattern
subscribes to channels and patterns (`SUBSCRIBE`, `PSUBSCRIBE`) and
receives messages published by other connections (`PUBLISH`), including
its own.

**Why this priority**: these change the connection itself rather than the
data — the driver's command pipeline and the subscription state — so they
are separate from the storage-layer stories.

**Independent Test**: Run the documented transaction and pub/sub workflows
with two or more stock clients over separate connections and verify queueing,
atomicity, watch abort, and message delivery.

**Acceptance Scenarios**:

1. **Given** a `MULTI` block queueing several commands, **When** `EXEC`
   is issued, **Then** all queued commands execute in order and their
   replies return in the documented multi-bulk arrangement; **When**
   `DISCARD` is issued instead, **Then** nothing executes and the
   connection returns to normal state.
2. **Given** a queued command that would fail at EXEC time (for example a
   `WRONGTYPE` write), **When** `EXEC` runs, **Then** the failing entry
   reports its error inline, subsequent queued commands still execute,
   and earlier successful writes remain applied (no rollback), matching
   Redis semantics; a queuing-time syntax error discards the whole block
   at `EXEC` as Redis does.
3. **Given** a `WATCH`ed key and a concurrent session modifying it between
   `WATCH` and `EXEC`, **When** `EXEC` is issued, **Then** the transaction
   aborts with the Redis null/pause reply and no queued command runs;
   after `UNWATCH` or a completed `EXEC`, subsequent `WATCH`es behave
   cleanly.
4. **Given** two connected clients, **When** one publishes to a channel,
   **Then** every client subscribed to that channel receives the message
   in the documented reply shape (channel and payload), and pattern
   subscribers via `PSUBSCRIBE` receive it with the original and matched
   pattern; unsubscribed clients receive nothing.
5. **Given** a subscriber, **When** it `UNSUBSCRIBE`s from all channels
   and the publish side continues, **Then** it receives no further
   messages and its connection remains usable for regular commands.

---

### User Story 4 — INFO and introspection (Priority: P2)

An operator or monitoring tool connects with a stock Redis client and runs
`INFO` (bare and per-section: `server`, `clients`, `memory`, `stats`,
`keyspace`, `replication`, and others as specified in the capability
table) and the companion inspection commands (`DBSIZE`, the documented
`SCAN` and `OBJECT` subset, `TYPE`, and the `COMMAND` summary line). The reply
is the Redis CRLF line format, and each reported value reflects the local
node and its namespace-visible data, matching the observability stats of
the product rather than a stock Redis's internals it does not have.

**Why this priority**: introspection makes the dialect verifiable and lets
existing Redis monitoring tooling consume the server; it depends on the
data stories but is independent of pub/sub and transactions.

**Independent Test**: With an unmodified stock client, call `INFO` with no
argument and with each documented section and verify format, section
coverage, and that counters move after known data operations.

**Acceptance Scenarios**:

1. **Given** a connected session, **When** `INFO` runs with no argument,
   **Then** the reply is the Redis CRLF `field:value` format containing
   each documented section with its first-level header line
   (`# section`).
2. **Given** a namespace with keys of known types, **When** `INFO
   keyspace` and `DBSIZE` run, **Then** the counts match the namespace's
   live, non-expired keys and the per-type breakdown matches what was
   created.
3. **Given** known data operations (writes, reads, expired deletions),
   **When** `INFO stats` is read before and after, **Then** the
   operations counters for that namespace increase by the expected
   amounts and map to the product's observability stats.
4. **Given** any key, **When** `TYPE` runs, **Then** the reply names the
   Redis-visible type (`string`, `hash`, `list`, `set`, `zset`, `stream`
   per the mapped SpaceStorage type) and matches what the write command
   created.

---

### User Story 5 — Bitmaps and HyperLogLog (Priority: P3)

A client uses the specialized string-derived and sketch types: bitmaps
(`SETBIT`, `GETBIT`, `BITCOUNT`, `BITPOS`, `BITOP`), which stay in scope
as a family (resource group 9), and HyperLogLog sketches (`PFADD`,
`PFCOUNT`, `PFMERGE`), which are backed by a registered foundation
primitive type rather than a private string encoding (FR-017).

**Why this priority**: real but narrower use cases; the bitmap family
rides on strings and the HyperLogLog family rides on a registered
cardinality-sketch primitive, and they can arrive after the main
families.

**Independent Test**: For each documented family, run the documented
commands with a stock client and verify reply values within the documented
accuracy limits.

**Acceptance Scenarios**:

1. **Given** bits set with `SETBIT` at scattered offsets, **When** the
   client reads with `GETBIT`, `BITCOUNT` (with range syntax), and
   `BITPOS`, and transforms with `BITOP` (`AND`/`OR`/`XOR`/`NOT` writing
   to a destination key), **Then** values match Redis semantics including
   missing-bit default 0 and `BITOP`'s length rule.
2. **Given** sketches populated with `PFADD`, **When** the client reads
   `PFCOUNT` and merges with `PFMERGE`, **Then** the estimated distinct
   count is within the documented HyperLogLog standard error and merged
   sketches behave like a sketch built from the union of inputs.
3. **Given** the registered HyperLogLog primitive type, **When** the type
   catalog is read, **Then** the cardinality-sketch type appears at level
   L0 with its accuracy bound, canonical representation, storage modes,
   and operation set, and the identical `PFADD`/`PFCOUNT`/`PFMERGE`
   behavior is reachable through the shared abstract type interface.

---

### Edge Cases

- A command is issued with the wrong number of arguments or wrong value
  type (for example a score that is not a number in `ZADD`): the Redis
  command-error form names the problem and no partial state change occurs.
- A command outside the capability table or a not-implemented option of a
  listed command (for example Redis functions/Lua `EVAL`, module
  commands, `CLUSTER` subcommands, aggregation-hidden features): reply is
  the Redis not-supported error form naming the command or option; no
  silent empty success.
- A key expires or is deleted mid-operation (for example between a
  `WATCH` and `EXEC`, or as a stream entry target): the operation sees
  the key as missing and replies accordingly; no stale data surfaces.
- Numeric limits: out-of-range offsets for bitmaps (bulk offset limit),
  absurd counts/limits, ID overflow at stream ID boundaries: Redis
  documented limit errors, not crashes or silent truncation.
- A `BLPOP`-style blocking command arrives on a connection that then
  blocks indefinitely (timeout 0): the block is honored while no
  administrator maximum block time is set; an administrator-configured
  cap overrides timeout 0 and longer explicit timeouts, after which the
  Redis timeout reply applies and the connection serves commands again;
  blocking commands queue with other commands per Redis connection rules.
- `SCAN`/`HSCAN`/`SSCAN`/`ZSCAN` iteration over a live dataset that is
  concurrently mutated: per documented scan guarantees (may return
  duplicates, misses later additions, always terminates with a full
  cursor round).
- Two connections publish and subscribe on the channel/pattern namespace
  bound to the credential's namespace only: messages never cross
  namespace boundaries; a subscriber cannot reach another tenant's
  channels.
- `INFO replication` on a single node and on a cluster member: reported
  role/counters reflect this node's real state, never a fabricated
  stock-Redis-looking constant.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The complete-product Redis dialect MUST be defined by a
  **capability table** owned by this feature, listing every supported
  Redis command family (the families of user stories 1–5) and each
  command's supported options. Every command in the table MUST work with
  unmodified stock Redis clients and return the documented Redis reply
  shapes.
- **FR-002**: The dialect MUST cover, at minimum: INFO and companion
  introspection (`DBSIZE`, `TYPE`, documented `SCAN`/`OBJECT` subset);
  Streams (`XADD`, `XREAD`, `XREADGROUP`, `XGROUP`, `XACK`, `XPENDING`,
  `XCLAIM`, `XLEN`, `XRANGE`, `XREVRANGE`, `XDEL`, `XTRIM`, `XINFO` with
  their documented options); lists, sets, sorted sets, and hashes with
  their full documented command sets as listed in the acceptance
  scenarios, hashes including the hash-field expiry family (`HEXPIRE`,
  `HPEXPIRE`, `HEXPIREAT`, `HPEXPIREAT`, `HTTL`, `HPTTL`,
  `HEXPIRETIME`, `HPEXPIRETIME`, `HGETEX`, `HGETDEL`); the expiry family
  (`SETEX`, `PSETEX`, `SET … EX/PX`,
  `EXPIRE`/`PEXPIRE`/`EXPIREAT`/`PEXPIREAT`, `TTL`/`PTTL`, `PERSIST`);
  pub/sub (`SUBSCRIBE`, `UNSUBSCRIBE`, `PSUBSCRIBE`, `PUNSUBSCRIBE`,
  `PUBLISH`, `PING` in subscribe mode); transactions (`MULTI`, `EXEC`,
  `DISCARD`, `WATCH`, `UNWATCH`); bitmaps (`SETBIT`,
  `GETBIT`, `BITCOUNT`, `BITPOS`, `BITOP`); HyperLogLog (`PFADD`,
  `PFCOUNT`, `PFMERGE`), whose state lives in the registered
  cardinality-sketch foundation primitive type (FR-017), not a
  protocol-private encoding.
- **FR-003**: Every Redis command MUST execute against the shared
  datatype abstraction through the shared execution stack; the Redis
  handler MUST NOT hide or re-tag the store's types and MUST NOT maintain
  a protocol-private type system. Redis keys MUST be typed entries inside
  the namespace's single default Redis key-space container (the Clarified
  model): a key's entry is created implicitly by the first write command
  that targets it, carries exactly one Redis-visible type per entry, and
  returns `WRONGTYPE` when another type's command targets the same key.
  The key-space container is one shared store object addressable from
  other protocols under the documented mapping of its entries.
- **FR-004**: Redis-visible types MUST map to the store's datatypes by a
  documented, lossless mapping; `TYPE` MUST report the Redis-visible name
  of the mapped type of the addressed key's entry, and a stock client
  MUST get the Redis reply shape regardless of how the key-space
  container or its entries were created or last modified from another
  protocol (shared-object promise of `02`).
- **FR-005**: Redis semantics MUST hold for each listed command: strict
  ID ordering and ID rules for Streams. Each stream key's ordering model
  is chosen at creation: total order (the default — one ID sequence all
  producers and observers agree on; writes serialize through a single
  writer per stream key whose designation is requested from the control
  plane, constitution `XII`, and group offsets derive from that order)
  or producer order (each producer's writes keep their own order;
  different producers' entries interleave arbitrarily); element order
  rules for lists;
  score/member ordering and inclusive/exclusive range syntax for sorted
  sets; set-operation semantics for sets; field-level semantics for
  hashes; per-key expiry timing and `TTL` reporting, plus per-field
  expiry timing for hash fields (`HEXPIRE`-family semantics of the 7.4
  baseline); queued-and-atomic
  execution with Redis's no-rollback EXEC behavior for transactions;
  at-most-once-per-group delivery model for Streams groups; documented
  accuracy/error bounds for HyperLogLog.
- **FR-006**: Blocking forms (`BLPOP`, `BRPOP`, `BZPOPMIN`, `BZPOPMAX`,
  `XREAD BLOCK`, `XREADGROUP BLOCK`) MUST block only their own connection, release on the
  triggering event or timeout, and return the Redis timeout reply; with
  timeout 0 the block MUST be honored indefinitely (stock semantics),
  unless an administrator-configured maximum block time is set, which
  then overrides timeout 0 and longer explicit timeouts; other
  connections and other commands on the same connection per Redis
  rules MUST NOT be blocked by one client's block.
- **FR-007**: Expiry MUST behave per key exactly as specified in the
  scenario: the key MUST be absent to everything after its expiry time
  and `TTL`/`PTTL` MUST report the remaining lifetime; hash fields with
  field TTL (`HEXPIRE` family) MUST behave the same way per field, with
  `HTTL`-family reporting and field reads after expiry behaving as
  absent; expiry MUST be consistent with the expiry model of the
  durability feature and per namespace quotas.
- **FR-008**: Transactions MUST be scoped per Redis connection with
  `WATCH` state maintained server-side; `EXEC` MUST apply all queued
  commands atomically from that connection's view; a `WATCH`ed key's
  change by any other session MUST abort the `EXEC` with the Redis null
  reply. Transactions MUST respect the isolation model the query
  execution feature defines; Redis has no isolation notion on the wire,
  so the documented default applies.
- **FR-009**: Pub/sub channels and patterns MUST be scoped inside the
  session's namespace (credential-bound per `02`); no message, channel
  list, or pattern list MAY cross a namespace boundary. Subscription
  state MUST be per connection; a subscriber connection's reply shapes
  in subscribe mode MUST match Redis exactly, including `PING` reply
  forms in subscribe mode.
- **FR-010**: `INFO` MUST reply in Redis CRLF format with the documented
  section set (at minimum `server`, `clients`, `memory`, `stats`,
  `keyspace`, `replication`) and MUST express this node's and namespace's
  real state mapped from the product's observability statistics; fields
  the product does not have internally MUST either be omitted or
  reported with documented values (never constant fakes).
- **FR-011**: Every command in the capability table MUST be listable and
  each not-supported command/option MUST be discoverable through
  documented introspection (`COMMAND`/`INFO` lines per the table) so
  tooling can see the dialect without trial-and-error.
- **FR-012**: Commands and options outside the capability table —
  including Lua/`EVAL`/functions, module commands, `CLUSTER`
  subcommands, Redis Sentinel roles, ACL management commands, the
  geospatial command family (`GEOADD`, `GEODIST`, `GEOPOS`, `GEOHASH`,
  `GEORADIUS`, `GEORADIUS_RO`, `GEORADIUSBYMEMBER`, `GEORADIUSBYMEMBER_RO`,
  `GEOSEARCH`, `GEOSEARCHSTORE`), and
  `XAUTOCLAIM` (user exclusions) — MUST return the Redis
  not-supported error form naming the command, without partial
  execution and without silent empty success.
- **FR-013**: Namespace, authentication, and session rules of the
  protocol feature (`02`) apply unchanged: credential-bound namespace for
  Redis sessions, per-session and per-query quorum/timeout through the
  documented Redis option mechanism, and error forms as Redis clients
  understand.
- **FR-014**: The capability table MUST stay within the limits the
  compatibility feature sets (key/value size caps, connection limits,
  admission limits, quotas); exceeding any documented limit MUST reject
  with a named error in Redis form.
- **FR-015**: Where this capability table conflicts with the Redis lines
  of the compatibility ceiling (`015`), this spec's table wins and the
  merged compatibility matrix MUST be updated accordingly; the
  first-binary Redis subset of `016` MUST remain unchanged by this
  feature (complete-product scope).
- **FR-016**: Stream consumer groups MUST obey the durability feature's
  persistence rules for their underlying entries: stream keys and their
  consumer groups in persistent storage MUST restore stream content,
  last-delivered positions, and group offsets across restart; entries in
  memory-mode storage MUST NOT promise content across process restart.
  Pub/sub messages are fire-and-forget and MUST NOT be persisted or
  replayed across restart (stock Redis semantics). A stock client MUST
  observe group offsets survive restarts of persistent streams as part
  of the durability contract.
- **FR-017**: HyperLogLog MUST be backed by a first-class foundation
  primitive type — a cardinality sketch at level L0, alongside
  `Bloom Filter` — registered through the extension path of the type
  system feature (`03` FR-045–FR-049) and reachable only through the
  shared abstract type interface; a protocol-private string encoding for
  the sketch is prohibited. The type's descriptor MUST carry the
  documented standard-error/accuracy bound of FR-005, its canonical
  self-describing versioned representation (`03` FR-042), its storage
  modes, and its directly-creatable flag, and `PFADD`/`PFCOUNT`/`PFMERGE`
  MUST map onto that type's `add`/`count`/`merge` operations. The `03` L0
  inventory (`03` FR-003; entry `hyperloglog`, `03` type-inventory
  "Registered extensions") is extended by this feature; existing types and
  containers are unchanged (`03` FR-046). The Redis-visible type reported
  for an HLL key remains `string`, per the lossless mapping of FR-004.

### Key Entities *(include if feature involves data)*

- **Redis Capability Table**: The complete-product list of supported Redis
  command families, commands, and options, plus the not-supported list.
  Owned by this feature; consumed by the dialect implementation and the
  conformance suite.
- **Default Key-Space Container**: The namespace's single shared store
  container that holds every Redis key as a typed entry; exists as one
  addressable object for the namespace, visible to other protocols under
  the documented mapping of its entries.
- **Redis-Visible Type**: The type name a stock client sees in `TYPE`
  (`string`, `hash`, `list`, `set`, `zset`, `stream`), mapped to the
  store's datatypes per entry; each key's entry carries exactly one such
  type, and `WRONGTYPE` protects against type mismatch between commands
  and entry type.
- **Stream Consumer Group**: A named position and delivery state over one
  stream key's entry: last-delivered ID, per-consumer pending entry lists
  (with idle time and delivery count), and acknowledgement state.
- **Subscription**: A connection-scoped channel- or pattern-scoped
  receiver inside one namespace, with subscribe-mode reply behavior.
- **Transaction Session**: The `MULTI` state of one connection: queued
  commands, `WATCH`ed key set, abort/exec decision.
- **Namespace-Scoped Channel Name**: A pub/sub channel or pattern
  interpreted inside the session's credential-bound namespace.
- **Key Expiry Record**: Per-key lifetime expressed as a point in time,
  shared with the expiry model of other protocols.
- **HyperLogLog Sketch Type**: The registered L0 foundation primitive
  (cardinality sketch) backing the HyperLogLog command family; its
  descriptor carries the accuracy bound, canonical representation,
  storage modes, and operation set (`add`, `count`, `merge`, `clear`), and
  its Redis-visible mapped type is `string` (FR-017).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of the commands in the Redis capability table pass
  their documented workflow cases in the conformance suite with
  unmodified stock Redis clients; 0 failures.
- **SC-002**: 100% of commands and options on the not-supported list
  return the Redis not-supported error form naming the command; 0 silent
  empty successes across the suite.
- **SC-003**: Streams: 100% of documented consumer-group cases in the
  suite deliver each entry to exactly one consumer at a time, preserve ID
  ordering, and match `XPENDING`/`XACK`/`XCLAIM` semantics; 0 lost or
  double-delivered (same group, same instant) cases in the suite.
- **SC-004**: Transactions: 100% of `WATCH`-abort and `EXEC`-error
  interleave cases in the suite match Redis semantics (abort on watched
  change, no rollback on inline error); 0 partial-state surprises.
- **SC-005**: Expiry: 100% of TTL round-trip cases in the suite observe
  the key absent after expiry and `TTL`/`PTTL` within the documented
  tolerance; 0 keys readable after expiry.
- **SC-006**: `INFO`: 100% of documented sections present in the
  required form; counters in the suite match the product's observability
  statistics within 1 operation of manual counts.
- **SC-007**: HyperLogLog estimates return within the documented standard
  error in 100% of suite cases; 100% of hash-field expiry cases in the
  suite observe fields absent after their field TTL and
  `HTTL`-family reporting matching the set lifetime. The cardinality-sketch
  primitive type appears in the type catalog with its accuracy bound, and
  every `PFADD`/`PFCOUNT`/`PFMERGE` case executes through the shared
  abstract type interface (`03` FR-040).
- **SC-008**: Exceeding any documented size, admission, or quota limit
  through a Redis command rejects with the limit named in the Redis error
  form in 100% of suite cases; 0 hangs.

## Assumptions

- **Complete-product scope.** This feature defines the complete-product
  Redis dialect; the first binary's Redis subset (intent `16`) is NOT
  changed or required by this feature. Verbs added here are additional to
  `16`, consistent with `02`/`015`'s layering.
- **Dialect baseline.** The command surface pins the Redis 7.4-line
  semantics for the listed families — hash-field expiry (`HEXPIRE`
  family, `HTTL` reports, `HGETEX`/`HGETDEL`) is in scope; the
  geospatial family and `XAUTOCLAIM` are user-excluded below that
  baseline. Any option newer than 7.4 or not in the capability table
  lands on the not-supported list — nothing in between.
- **Excluded subsystems stay excluded.** Lua/scripting (`EVAL`/functions),
  modules, Redis Cluster sharding (`CLUSTER`, slot redirection), Sentinel
  mode, geospatial commands, `XAUTOCLAIM`, and Redis ACL-management
  commands remain out of scope and
  not-supported, consistent with `015`'s "modules, Lua" exclusions and
  SpaceStorage's own role system (`07`/`14`).
- **Persistence follows the container's tier, not the command.** All
  Redis keys live in the namespace's single default key-space container
  (Clarified model), so every entry in a namespace shares that
  container's tier (memory/hybrid/persistent per the storage feature): a
  persistent tier's streams and their consumer-group offsets survive
  restarts; a memory-mode tier does not promise that.
- **INFO maps the product's real stats.** `INFO` sections are populated
  from the product's observability model; fields with no internal
  equivalent are omitted or carry a documented value, so monitoring sees
  truth that maps to the product's metrics rather than a life-like mask.
- **One namespace per session.** Pub/sub, keys, and transactions are all
  namespace-scoped to the credential-bound namespace; no cross-namespace
  behavior exists at this layer.
- **HyperLogLog is a registered type, not a private encoding.** The
  HyperLogLog family is backed by a first-class L0 cardinality-sketch
  primitive registered through `03`'s extension path (FR-017); this
  feature carries that inventory extension and the type's descriptor, and
  it does not add any protocol-private type system. The bitmap family
  likewise rides on the existing `Bitmap / Bitset` primitive rather than
  a private encoding.

## Dependencies

- Protocols and namespace/credential binding rules (`specs/002`).
- Shared datatype abstraction and type mapping (`specs/003`, `002`).
- Type system inventory extension for the HyperLogLog cardinality-sketch
  primitive (`specs/003` FR-002/FR-045–FR-049; registered extension
  `hyperloglog`).
- Query execution stack (`specs/005`).
- Durability and persistence tiers incl. stream/group restoration
  (`specs/013`).
- First-binary Redis subset boundary (`specs/016`).
- Compatibility ceiling conflict resolution (`specs/015`; per FR-015 this
  spec wins for the Redis dialect on merge).
