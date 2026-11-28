# Redis dialect (`017`)

The SpaceStorage node speaks RESP2 on the Redis port. Two surfaces exist:

- **First binary** (`016`, default) — the frozen K/V string subset: `AUTH`,
  `PING`, `GET`, `SET`, `DEL`, `EXISTS`, `SCAN`, `SELECT`, `TTL`, `PTTL`,
  `EXPIRE`. This page does not change it.
- **Complete product** (`017`) — the full command surface below, served when the
  node runs the `CompleteProduct` dialect profile.

Authoritative machine artifacts live in
[`specs/017-redis-dialect-coverage/contracts/`](../specs/017-redis-dialect-coverage/contracts/):
[`capability-table.md`](../specs/017-redis-dialect-coverage/contracts/capability-table.md),
[`not-supported.md`](../specs/017-redis-dialect-coverage/contracts/not-supported.md),
[`redis-mappings.md`](../specs/017-redis-dialect-coverage/contracts/redis-mappings.md).
The tables below summarize them for operators.

## Key-space model

A session is bound to exactly one namespace by its credential (`002`). All Redis
keys live in that namespace's single key-space container (default name `redis`,
schema `public`, store type L2 `kv_collection`). Every Redis key is one typed
entry in that container:

- Entry type is fixed by the first write command that targets the key
  (`SET` → `string`, `HSET` → `hash`, `LPUSH` → `list`, `SADD` → `set`,
  `ZADD` → `zset`, `XADD` → `stream`, `SETBIT` → bitmap-as-`string`,
  `PFADD` → HyperLogLog-as-`string`).
- A command of a different type against the same key returns
  `-WRONGTYPE Operation against a key holding the wrong kind of value` and
  writes nothing.
- `TYPE` reports the Redis-visible type. `bitmap` and `hyperloglog` report
  `string`.
- The container is the same object other protocols address through the `002`
  mapping, so a value written over Redis is visible to the canonical
  `SS.GET`/PostgreSQL point read on the same container (`017` FR-003).

## Capability table (`MUST` on complete product)

Anything not in this table is refused with the not-supported error. Options
shown are accepted; an unknown option of a listed command is likewise refused.

| Group | Commands |
|-------|----------|
| Connection / server meta | `AUTH`, `PING`, `ECHO`, `QUIT`, `SELECT` (index 0 only), `COMMAND [INFO|COUNT]`, `CLIENT {ID,INFO,LIST,KILL,SETNAME,GETNAME,PAUSE}`, `CONFIG {GET,SET}`, `TIME` |
| Keyspace introspection | `INFO [section]`, `DBSIZE`, `TYPE`, `TTL`, `PTTL`, `EXISTS`, `SCAN`, `RANDOMKEY`, `OBJECT {ENCODING,IDLETIME}`, `TOUCH`, `UNLINK` |
| Strings + expiry | `GET`, `SET` (`NX`/`XX`/`EX`/`PX`/`KEEPTTL`), `DEL`, `MGET`, `MSET`, `MSETNX`, `SETNX`, `SETEX`, `PSETEX`, `GETSET`, `STRLEN`, `APPEND`, `GETRANGE`, `SETRANGE`, `INCR`, `DECR`, `INCRBY`, `DECRBY`, `INCRBYFLOAT`, `GETDEL`, `GETEX`, `EXPIRE`, `PEXPIRE`, `EXPIREAT`, `PEXPIREAT`, `PERSIST`, `EXPIRETIME`, `PEXPIRETIME` |
| Hash (incl. field expiry) | `HSET`, `HGET`, `HMSET`, `HMGET`, `HGETALL`, `HKEYS`, `HVALS`, `HLEN`, `HEXISTS`, `HDEL`, `HSETNX`, `HINCRBY`, `HINCRBYFLOAT`, `HSTRLEN`, `HRANDFIELD`, `HSCAN`, `HEXPIRE`, `HPEXPIRE`, `HEXPIREAT`, `HPEXPIREAT`, `HEXPIRETIME`, `HPEXPIRETIME`, `HTTL`, `HPTTL`, `HGETEX`, `HGETDEL` |
| List | `LPUSH`, `RPUSH`, `LPOP`, `RPOP`, `LPUSHX`, `RPUSHX`, `LLEN`, `LRANGE`, `LINDEX`, `LSET`, `LREM`, `LTRIM`, `LINSERT`, `RPOPLPUSH`, `LMOVE`, `BLPOP`, `BRPOP`, `BRPOPLPUSH`, `BLMOVE` |
| Set | `SADD`, `SREM`, `SISMEMBER`, `SMEMBERS`, `SCARD`, `SPOP`, `SRANDMEMBER`, `SSCAN`, `SUNION`, `SUNIONSTORE`, `SINTER`, `SINTERSTORE`, `SDIFF`, `SDIFFSTORE` |
| Sorted set | `ZADD` (`NX`/`XX`/`GT`/`LT`/`CH`/`INCR`), `ZREM`, `ZCARD`, `ZSCORE`, `ZMSCORE`, `ZCOUNT`, `ZRANK`, `ZREVRANK`, `ZRANGE`, `ZREVRANGE`, `ZRANGEBYSCORE`, `ZREVRANGEBYSCORE`, `ZRANGESTORE`, `ZPOPMIN`, `ZPOPMAX`, `BZPOPMIN`, `BZPOPMAX`, `ZUNION`, `ZINTER`, `ZDIFF`, `ZUNIONSTORE`, `ZINTERSTORE`, `ZDIFFSTORE`, `ZSCAN`, `ZINCRBY`, `ZREMRANGEBYSCORE`, `ZREMRANGEBYRANK` |
| Streams + groups | `XADD`, `XDEL`, `XRANGE`, `XREVRANGE`, `XLEN`, `XTRIM`, `XREAD`, `XREADGROUP`, `XGROUP {CREATE,DESTROY,SETID,DELCONSUMER,CREATECONSUMER,HELP}`, `XACK`, `XPENDING`, `XCLAIM`, `XINFO {STREAM,GROUPS,CONSUMERS,HELP}` |
| Bitmaps | `SETBIT`, `GETBIT`, `BITCOUNT`, `BITPOS`, `BITOP`, `BITFIELD`, `BITFIELD_RO` |
| HyperLogLog | `PFADD`, `PFCOUNT`, `PFMERGE` |
| Pub/Sub (connection-scoped) | `SUBSCRIBE`, `UNSUBSCRIBE`, `PSUBSCRIBE`, `PUNSUBSCRIBE`, `PUBLISH`, `PUBSUB {CHANNELS,NUMSUB,NUMPAT}`, `SPUBLISH`, `SSUBSCRIBE`, `SUNSUBSCRIBE` |
| Transactions (connection-scoped) | `MULTI`, `EXEC`, `DISCARD`, `WATCH`, `UNWATCH` |

`COMMAND` lists exactly the rows above (`COMMAND COUNT` returns their number).

### Blocking commands

`BLPOP`, `BRPOP`, `BRPOPLPUSH`, `BLMOVE`, `BZPOPMIN`, `BZPOPMAX`, `XREAD BLOCK`,
and `XREADGROUP BLOCK` wait on an in-process notification (never a worker-thread
sleep) until an element/entry arrives, the client timeout elapses, or the
connection closes. The server-side upper bound is `protocols.redis.max_block`
(see below).

## Not-supported list

Every command below returns an error and never a `+OK`/empty-array success. The
generic form is `-ERR unknown command '<COMMAND>'`.

| Family | Commands |
|--------|----------|
| Geospatial | `GEOADD`, `GEOPOS`, `GEODIST`, `GEORADIUS`, `GEORADIUS_RO`, `GEORADIUSBYMEMBER`, `GEORADIUSBYMEMBER_RO`, `GEOSEARCH`, `GEOSEARCHSTORE`, `GEOHASH` |
| Stream auto-claim | `XAUTOCLAIM` (`XCLAIM` is supported) |
| Lua / scripting | `EVAL`, `EVALSHA`, `SCRIPT` |
| Functions | `FUNCTION`, `FCALL`, `FCALL_RO` |
| Modules | `MODULE` |
| Cluster | `CLUSTER` (except `CLUSTER INFO`/`CLUSTER NODES`, which may reply "cluster disabled" info) |
| Sentinel | `SENTINEL` |
| ACL management | `ACL` (credentials are managed by `002`/`014`; simple `AUTH password` stays supported) |
| Historical / deprecated | `DEBUG`, `SHUTDOWN`, `SLAVEOF`, `REPLICAOF`, `SYNC`, `PSYNC`, `MIGRATE`, `RESTORE`, `DUMP`, `RENAME`, `RENAMENX` |
| RESP3-only | `HELLO` (RESP2 only), `CLIENT SETINFO` |
| Database selection / swap | `SWAPDB`; `SELECT n>0` |

A bad subcommand of a listed command takes
`-ERR unknown subcommand or wrong number of arguments`.

`SELECT 0` is a no-op `MUST` (the namespace is already bound by the credential).
`SELECT n>0` is an explicit exception with a named form:

```
-ERR SELECT is not supported: namespaces are bound to credentials
```

## Redis ↔ store type mapping

| Redis-visible type | Internal type | Level |
|--------------------|---------------|-------|
| `string` | container entry value (canonical encoding) | L2 |
| `string` (Bitmap) | L0 `bitmap` | L0 |
| `string` (HyperLogLog) | L0 `hyperloglog` (registered extension, default precision 14, ≈0.81% error) | L0 |
| `hash` | L0 `hash_table` | L0 |
| `list` | L0 `linked_list`/`deque` | L0 |
| `set` | L0 `hash_table` (value-less) | L0 |
| `zset` | L0 `skip_list` index + `hash_table` member→score map | L0 |
| `stream` | L0 `ring_buffer` + `append_segment` | L0 |
| `none` | no entry | — |

Stream keys choose an ordering model at creation: total order (default; one
cluster-wide ID sequence, gated by the `006` leadership lease) or producer order
(per-producer monotonicity, no lease). The model is fixed on the first write.

## Configuration

| Knob | Type | Default | Meaning |
|------|------|---------|---------|
| `protocols.redis.max_block` | duration | **unset** | Server-side cap on blocking commands. Unset means the stock Redis timeout `0` (no server-side cap; the client timeout governs). A negative value fails validation with a named `max_block` error. |

`CONFIG GET max_block` returns the configured value when set and an empty array
when unset; `CONFIG SET max_block <seconds>` updates it for the connection.
`015` size limits still apply: `limits.max_key` (default 1 KiB) and
`limits.max_value` (default 16 MiB) reject oversized keys/values with a named
`limit_exceeded{limit:key|value,…}` error before any state is written; bit
offsets at or above `2^32` are out of range.

## Verification

```sh
cargo test -p spacestorage-conformance --features redis-dialect
cargo test -p spacestorage-compat
cargo test -p spacestorage-handler-redis
```

The complete-product conformance suites are `redis_not_supported`,
`redis_limits`, `redis_collections`, `redis_streams`, `redis_txn_pubsub`,
`redis_info`, `redis_bitmap_hll`, `redis_binary_safety`, and `dialect_redis`
(the last also pins that the first-binary subset gains no new `MUST` verb).
