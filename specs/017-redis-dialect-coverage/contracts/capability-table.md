# Contract: Redis Capability Table

**Feature**: `017-redis-dialect-coverage` | Spec: FR-001, FR-002, FR-011, FR-012

The complete-product Redis dialect surface. All commands listed here are `MUST` on the `CompleteProduct` profile; everything else is `MUST NOT` (returns the Redis not-supported error form).

## Legend

- **R** = Resource group number (from spec FR-002)
- **Blocking** = Yes if command can block (has timeout semantics)
- **Tx allowed** = Yes if command can be used inside `MULTI`/`EXEC`

---

## R1: Connection/Server Meta

| Command | R | Blocking | Tx allowed | Notes |
|---------|---|----------|------------|-------|
| `AUTH` | 1 | No | No | Credential validation per `002` |
| `PING` | 1 | No | Yes | Returns `PONG` or payload echo |
| `ECHO` | 1 | No | Yes | Returns argument unchanged |
| `QUIT` | 1 | No | No | Graceful connection close |
| `SELECT` | 1 | No | No | Only `SELECT 0` (no-op); `SELECT n>0` not-supported (see [not-supported.md](not-supported.md) §11) |
| `COMMAND` | 1 | No | Yes | List capability table commands per FR-011 |
| `COMMAND INFO` | 1 | No | Yes | Metadata for listed commands |
| `COMMAND COUNT` | 1 | No | Yes | Count of capability table commands |
| `CLIENT *` | 1 | No | No | See subset below |
| `CONFIG GET` | 1 | No | No | Read mapping: `max_block` only; others omitted |
| `CONFIG SET` | 1 | No | No | `max_block`; returns error for others |
| `TIME` | 1 | No | Yes | Server timestamp (reads the `012` HLC / server clock; no lease) |

`SWAPDB` is not-supported (see [not-supported.md](not-supported.md) §11); it is not a `MUST` command.

**CLIENT subset**: `CLIENT ID`, `CLIENT INFO`, `CLIENT LIST`, `CLIENT KILL` (by ID), `CLIENT SETNAME`, `CLIENT GETNAME`, `CLIENT PAUSE` → yes; others → not-supported.

---

## R2: Keyspace Introspection

| Command | R | Blocking | Tx allowed | Notes |
|---------|---|----------|------------|-------|
| `INFO` | 2 | No | No | Per FR-010; sections: `server`, `clients`, `memory`, `stats`, `keyspace`, `replication` |
| `INFO keyspace` | 2 | No | No | Keys/expiry count for this namespace only |
| `DBSIZE` | 2 | No | Yes | Count live keys in the namespace key-space (FR-002 introspection) |
| `TYPE` | 2 | No | Yes | Returns Redis-visible type per FR-003 |
| `TTL` | 2 | No | Yes | Seconds to expiry, -1 = none, -2 = key not exist |
| `PTTL` | 2 | No | Yes | Milliseconds to expiry |
| `EXISTS` | 2 | No | Yes | Count of existing keys |
| `SCAN` | 2 | No | Yes | Incremental iterator over key-space |
| `RANDOMKEY` | 2 | No | Yes | Returns random key or nil |
| `OBJECT ENCODING` | 2 | No | Yes | Internal layout hint |
| `OBJECT IDLETIME` | 2 | No | Yes | Idle time in seconds |
| `TOUCH` | 2 | No | Yes | Updates LRU/LFU in bulk |
| `UNLINK` | 2 | No | Yes | Async delete marker (immediate in memory mode) |

**OBJECT subset**: only `OBJECT ENCODING` and `OBJECT IDLETIME` are supported; every other `OBJECT` subcommand (`FREQ`, `REFCOUNT`, `HELP`) is not-supported per FR-012 (return the Redis not-supported error form).

---

## R3: Strings + Expiry

| Command | R | Blocking | Tx allowed | Notes |
|---------|---|----------|------------|-------|
| `GET` | 3 | No | Yes | `016` subset |
| `SET` | 3 | No | Yes | `016` subset; `NX`/`XX`/`EX`/`PX`/`KEEPTTL` |
| `DEL` | 3 | No | Yes | `016` subset |
| `MGET` | 3 | No | Yes | Multi-key get |
| `MSET` | 3 | No | Yes | Multi-key set |
| `MSETNX` | 3 | No | Yes | All-or-nothing multi-set |
| `SETNX` | 3 | No | Yes | Set if not exists |
| `SETEX` | 3 | No | Yes | Set with expiry in seconds |
| `PSETEX` | 3 | No | Yes | Set with expiry in ms |
| `GETSET` | 3 | No | Yes | Atomic get-then-set |
| `STRLEN` | 3 | No | Yes | Byte length |
| `APPEND` | 3 | No | Yes | Append bytes |
| `GETRANGE` | 3 | No | Yes | Substring by byte range |
| `SETRANGE` | 3 | No | Yes | Overwrite at offset |
| `INCR` | 3 | No | Yes | Atomic integer increment |
| `DECR` | 3 | No | Yes | Atomic integer decrement |
| `INCRBY` | 3 | No | Yes | Increment by amount |
| `DECRBY` | 3 | No | Yes | Decrement by amount |
| `INCRBYFLOAT` | 3 | No | Yes | Float increment |
| `GETDEL` | 3 | No | Yes | Get and delete atomically |
| `GETEX` | 3 | No | Yes | Get and adjust expiry |
| `EXPIRE` | 3 | No | Yes | Set TTL in seconds |
| `PEXPIRE` | 3 | No | Yes | Set TTL in milliseconds |
| `EXPIREAT` | 3 | No | Yes | Set expiry to Unix timestamp |
| `PEXPIREAT` | 3 | No | Yes | Set expiry to Unix ms timestamp |
| `PERSIST` | 3 | No | Yes | Remove TTL |
| `EXPIRETIME` | 3 | No | Yes | Return expiry as Unix timestamp |
| `PEXPIRETIME` | 3 | No | Yes | Return expiry as Unix ms timestamp |

---

## R4: Hash (incl. hash-field expiry)

| Command | R | Blocking | Tx allowed | Notes |
|---------|---|----------|------------|-------|
| `HSET` | 4 | No | Yes | Set fields |
| `HGET` | 4 | No | Yes | Get single field |
| `HMSET` | 4 | No | Yes | Set multiple |
| `HMGET` | 4 | No | Yes | Get multiple |
| `HGETALL` | 4 | No | Yes | All field/value pairs |
| `HKEYS` | 4 | No | Yes | All field names |
| `HVALS` | 4 | No | Yes | All values |
| `HLEN` | 4 | No | Yes | Field count |
| `HEXISTS` | 4 | No | Yes | Field exists? |
| `HDEL` | 4 | No | Yes | Delete fields |
| `HSETNX` | 4 | No | Yes | Set if field not exists |
| `HINCRBY` | 4 | No | Yes | Integer field incr |
| `HINCRBYFLOAT` | 4 | No | Yes | Float field incr |
| `HSTRLEN` | 4 | No | Yes | Value byte length |
| `HRANDFIELD` | 4 | No | Yes | Random field(s) |
| `HSCAN` | 4 | No | Yes | Iterates fields |
| `HEXPIRE` | 4 | No | Yes | Per-field expiry (sec) |
| `HPEXPIRE` | 4 | No | Yes | Per-field expiry (ms) |
| `HEXPIRETIME` | 4 | No | Yes | Get per-field expiry (sec) |
| `HPEXPIRETIME` | 4 | No | Yes | Get per-field expiry (ms) |
| `HEXPIREAT` | 4 | No | Yes | Per-field expiry timestamp (sec) |
| `HPEXPIREAT` | 4 | No | Yes | Per-field expiry timestamp (ms) |
| `HTTL` | 4 | No | Yes | Get per-field TTL (sec) |
| `HPTTL` | 4 | No | Yes | Get per-field TTL (ms) |
| `HGETEX` | 4 | No | Yes | Get field(s), optionally setting expiry |
| `HGETDEL` | 4 | No | Yes | Get field(s) and delete them |

---

## R5: List

| Command | R | Blocking | Tx allowed | Notes |
|---------|---|----------|------------|-------|
| `LPUSH` | 5 | No | Yes | Push to left |
| `RPUSH` | 5 | No | Yes | Push to right |
| `LPOP` | 5 | No | Yes | Pop from left |
| `RPOP` | 5 | No | Yes | Pop from right |
| `LPUSHX` | 5 | No | Yes | Push left only if list exists |
| `RPUSHX` | 5 | No | Yes | Push right only if list exists |
| `LLEN` | 5 | No | Yes | List length |
| `LRANGE` | 5 | No | Yes | Return slice by index |
| `LINDEX` | 5 | No | Yes | Element at index |
| `LSET` | 5 | No | Yes | Set element at index |
| `LREM` | 5 | No | Yes | Remove elements matching value |
| `LTRIM` | 5 | No | Yes | Trim to range |
| `LINSERT` | 5 | No | Yes | Insert before/after pivot |
| `RPOPLPUSH` | 5 | No | Yes | Pop right, push left to dest |
| `BLPOP` | 5 | **Yes** | No | Block until list has element or timeout |
| `BRPOP` | 5 | **Yes** | No | Block until list has element or timeout |
| `BRPOPLPUSH` | 5 | **Yes** | No | Block + pop/push |
| `LMOVE` | 5 | No | Yes | Pop from one list, push to another |
| `BLMOVE` | 5 | **Yes** | No | Blocking variant of LMOVE |

**Blocking behavior**: per FR-006; timeout 0 means indefinite unless `max_block` configured.

---

## R6: Set

| Command | R | Blocking | Tx allowed | Notes |
|---------|---|----------|------------|-------|
| `SADD` | 6 | No | Yes | Add members |
| `SREM` | 6 | No | Yes | Remove members |
| `SISMEMBER` | 6 | No | Yes | Member exists? |
| `SMEMBERS` | 6 | No | Yes | All members |
| `SCARD` | 6 | No | Yes | Cardinality |
| `SPOP` | 6 | No | Yes | Remove+return random member(s) |
| `SRANDMEMBER` | 6 | No | Yes | Return random member(s) without removing |
| `SSCAN` | 6 | No | Yes | Iterate members |
| `SUNION` | 6 | No | Yes | Union (no dest) |
| `SUNIONSTORE` | 6 | No | Yes | Union with destination |
| `SINTER` | 6 | No | Yes | Intersection (no dest) |
| `SINTERSTORE` | 6 | No | Yes | Intersection with destination |
| `SDIFF` | 6 | No | Yes | Difference (no dest) |
| `SDIFFSTORE` | 6 | No | Yes | Difference with destination |

---

## R7: Sorted Set

| Command | R | Blocking | Tx allowed | Notes |
|---------|---|----------|------------|-------|
| `ZADD` | 7 | No | Yes | Add with score |
| `ZREM` | 7 | No | Yes | Remove members |
| `ZCARD` | 7 | No | Yes | Cardinality |
| `ZSCORE` | 7 | No | Yes | Score of member |
| `ZMSCORE` | 7 | No | Yes | Scores of members |
| `ZCOUNT` | 7 | No | Yes | Count in score range |
| `ZRANK` | 7 | No | Yes | Rank (0-based low to high) |
| `ZREVRANK` | 7 | No | Yes | Rank (high to low) |
| `ZRANGE` | 7 | No | Yes | Members by rank range |
| `ZREVRANGE` | 7 | No | Yes | Reverse rank range |
| `ZRANGEBYSCORE` | 7 | No | Yes | Members by score range |
| `ZREVRANGEBYSCORE` | 7 | No | Yes | Reverse score range |
| `ZPOPMIN` | 7 | No | Yes | Pop lowest-scored member(s) |
| `ZPOPMAX` | 7 | No | Yes | Pop highest-scored member(s) |
| `BZPOPMIN` | 7 | **Yes** | No | Blocking pop lowest |
| `BZPOPMAX` | 7 | **Yes** | No | Blocking pop highest |
| `ZUNIONSTORE` | 7 | No | Yes | Union with destination |
| `ZINTERSTORE` | 7 | No | Yes | Intersection with destination |
| `ZDIFFSTORE` | 7 | No | Yes | Difference with destination |
| `ZUNION` | 7 | No | Yes | Union (no dest) |
| `ZINTER` | 7 | No | Yes | Intersection (no dest) |
| `ZDIFF` | 7 | No | Yes | Difference (no dest) |
| `ZSCAN` | 7 | No | Yes | Iterate members |
| `ZINCRBY` | 7 | No | Yes | Increment member score |
| `ZREMRANGEBYSCORE` | 7 | No | Yes | Remove by score range |
| `ZREMRANGEBYRANK` | 7 | No | Yes | Remove by rank range |
| `ZRANGESTORE` | 7 | No | Yes | Copy range to destination |

---

## R8: Streams + Consumer Groups

Stream ordering mode is per-key (total-order default, producer-order optional). See FR-005.

| Command | R | Blocking | Tx allowed | Notes |
|---------|---|----------|------------|-------|
| `XADD` | 8 | No | Yes | Append entry; ID `*` for auto |
| `XDEL` | 8 | No | Yes | Delete entries by ID |
| `XRANGE` | 8 | No | Yes | Range by ID |
| `XREVRANGE` | 8 | No | Yes | Reverse range |
| `XLEN` | 8 | No | Yes | Entry count |
| `XTRIM` | 8 | No | Yes | Trim by maxlen/minid |
| `XREAD` | 8 | **Yes** | No | Read from stream(s); `BLOCK` enables blocking |
| `XREADGROUP` | 8 | **Yes** | No | Read as consumer group member |
| `XGROUP CREATE` | 8 | No | Yes | Create consumer group |
| `XGROUP DESTROY` | 8 | No | Yes | Destroy group |
| `XGROUP SETID` | 8 | No | Yes | Set group last ID |
| `XGROUP DELCONSUMER` | 8 | No | Yes | Remove consumer from group |
| `XGROUP CREATECONSUMER` | 8 | No | Yes | Create consumer if absent |
| `XGROUP HELP` | 8 | No | Yes | Help text |
| `XACK` | 8 | No | Yes | Acknowledge PEL entries |
| `XPENDING` | 8 | No | Yes | View pending entries summary |
| `XCLAIM` | 8 | No | Yes | Claim entries from another consumer |
| `XINFO STREAM` | 8 | No | Yes | Stream metadata |
| `XINFO GROUPS` | 8 | No | Yes | List groups |
| `XINFO CONSUMERS` | 8 | No | Yes | List consumers in group |

**Not supported**: `XAUTOCLAIM` (per FR-012; return not-supported error).

---

## R9: Bitmaps

Bitmaps use the L0 `bitmap` primitive; Redis reports type `string` (FR-004).

| Command | R | Blocking | Tx allowed | Notes |
|---------|---|----------|------------|-------|
| `SETBIT` | 9 | No | Yes | Set bit at offset |
| `GETBIT` | 9 | No | Yes | Get bit at offset |
| `BITCOUNT` | 9 | No | Yes | Count set bits in range |
| `BITPOS` | 9 | No | Yes | First set/clear bit position |
| `BITOP` | 9 | No | Yes | AND/OR/XOR/NOT between keys |
| `BITFIELD` | 9 | No | Yes | Read/write packed integers |
| `BITFIELD_RO` | 9 | No | Yes | Read-only variant |

---

## HyperLogLog

HyperLogLog uses the registered L0 `hyperloglog` primitive (FR-017); Redis reports type `string`.

| Command | R | Blocking | Tx allowed | Notes |
|---------|---|----------|------------|-------|
| `PFADD` | 9 | No | Yes | Add elements to HLL |
| `PFCOUNT` | 9 | No | Yes | Approximate cardinality |
| `PFMERGE` | 9 | No | Yes | Merge HLLs into dest |

**Not supported**: `PFDEBUG` (not in FR-002's HLL family) falls under "everything else MUST NOT" and returns the not-supported error form.

---

## Pub/Sub (connection-scoped)

| Command | R | Blocking | Tx allowed | Notes |
|---------|---|----------|------------|-------|
| `SUBSCRIBE` | pubsub | No | No | Enter subscribe mode; blocking read semantics |
| `UNSUBSCRIBE` | pubsub | No | No | Leave channel(s) |
| `PSUBSCRIBE` | pubsub | No | No | Pattern subscribe |
| `PUNSUBSCRIBE` | pubsub | No | No | Pattern unsubscribe |
| `PUBLISH` | pubsub | No | Yes | Publish message |
| `PUBSUB *` | pubsub | No | No | `CHANNELS`, `NUMSUB`, `NUMPAT` allowed |
| `SPUBLISH` | pubsub | No | Yes | Sharded publish (shard ignored in single-node) |
| `SSUBSCRIBE` | pubsub | No | No | Sharded subscribe (shard ignored) |
| `SUNSUBSCRIBE` | pubsub | No | No | Sharded unsubscribe |

**Behavior**: Namespace-scoped (FR-009). Fire-and-forget; not persisted/replayed on reconnect.

---

## Transactions (connection-scoped)

| Command | R | Tx allowed | Notes |
|---------|---|------------|-------|
| `MULTI` | tx | N/A | Start transaction batch |
| `EXEC` | tx | N/A | Execute batch |
| `DISCARD` | tx | N/A | Abort batch |
| `WATCH` | tx | No | Optimistic locking keys |
| `UNWATCH` | tx | No | Clear watched keys |

**Behavior**: FR-008. No rollback on exec failure; `WATCH` aborts if key changed.

---

## Not-Supported Commands (FR-012)

The complete, closed not-supported list is canonical in [not-supported.md](not-supported.md) and is **not restated here** (research R11). By family it covers: geospatial (incl. the `_RO` aliases and `GEOSEARCHSTORE`), `XAUTOCLAIM`, Lua/scripting (`EVAL`/`EVALSHA`/`SCRIPT`), Functions (`FCALL`/`FCALL_RO`/`FUNCTION`), Modules, Cluster, Sentinel, ACL management (`AUTH` stays `MUST`), database selection / swap (`SELECT n>0`, `SWAPDB`), historical/deprecated replication and admin verbs (`MIGRATE`, `REPLICAOF`, `SLAVEOF`, `SYNC`, `PSYNC`, `DEBUG`, `RESTORE`, `DUMP`, `RENAME`, …), and RESP3-only features.

Invoking any of them returns `-ERR unknown command '<COMMAND>'` (or the documented subcommand / `002` form) with no partial execution and no silent empty success. `CLUSTER INFO`/`CLUSTER NODES` may reply with "cluster disabled" info; that does not imply Cluster support.

---

## Capability Count

Per FR-011: listable via `COMMAND`. Current complete-product count, counted from the command rows in the tables above: **183 command rows**; **180** excluding the `COMMAND` family itself (`COMMAND`, `COMMAND INFO`, `COMMAND COUNT`).

First-binary (`016`) count stays fixed at the `K/V Store` subset: **11 commands** — `AUTH`, `PING`, `GET`, `SET`, `DEL`, `EXISTS`, `SCAN`, `SELECT` (no-op), `TTL`, `PTTL`, `EXPIRE` (research R1; `016` [dialect-first-binary.md](../../016-mvp-and-nongoals/contracts/dialect-first-binary.md), `015` `matrix.md`).
