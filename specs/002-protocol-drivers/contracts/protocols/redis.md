# Protocol Contract: `redis`

**Crate**: `crates/handler-redis` | **Stack**: `redis-protocol` (RESP2/RESP3 codec) | Research: R4

| Item | Value |
|------|-------|
| Wire version | **RESP2** (default) and **RESP3** (`HELLO 3`); inline commands accepted; pipelining supported; `CLIENT TRACKING`/pub-sub not in v1 |
| Signature (FR-004) | first byte `*` (array) or printable ASCII inline command terminated by CRLF within 1 s; otherwise `-ERR unknown protocol` and close |
| Authentication | `AUTH <user> <pass>` (ACL style) or `AUTH <pass>` (user `default`) or `HELLO 3 AUTH user pass`; every other command before auth → `-NOAUTH Authentication required`; `Authenticator::Password` |
| Namespace / schema | credential-bound (FR-009a); `SELECT 0` accepted, `SELECT n>0` → `-ERR SELECT is not supported: namespaces are bound to credentials`; schema `public` |
| Target container | native commands operate on the session's target container — default `protocols.redis.default_container` (`redis`, `kv_collection`, auto-created); `SS.USE <container>` switches (container must exist and be `kv_collection` for native commands; other types accept only `SS.*` verbs) |
| Session identity | `CLIENT SETNAME`/`HELLO … SETNAME` → `client_app`; `CLIENT ID/GETNAME/INFO/LIST` (this connection) |
| Type option | `SS.CREATE <name> TYPE <type> [<opt> <value> …]` — type mandatory (no default for `SS.CREATE`); `SS.ALTER <name> <opt> <value> …`; `SS.DROP <name>` |
| Listing (FR-013) | `SS.CONTAINERS` → array of arrays `[name type created_via created_at]`; RESP3: array of maps |
| Native mappings | `kv_collection` ↔ string and hash commands (`GET SET SETNX SETEX PSETEX GETSET GETDEL MGET MSET MSETNX DEL UNLINK EXISTS TYPE INCR INCRBY INCRBYFLOAT DECR DECRBY APPEND STRLEN GETRANGE SETRANGE EXPIRE PEXPIRE EXPIREAT PEXPIREAT TTL PTTL PERSIST KEYS SCAN RANDOMKEY RENAME RENAMENX DBSIZE FLUSHDB HSET HSETNX HGET HMGET HMSET HGETALL HDEL HEXISTS HLEN HKEYS HVALS HINCRBY HINCRBYFLOAT HSCAN HSTRLEN`); `SET` options `EX PX EXAT PXAT NX XX KEEPTTL GET` |
| Canonical carrier | bulk string (raw canonical bytes) via `SS.GET <container> <key>`, `SS.PUT <container> <key> <canonical>`, `SS.DEL <container> <key>`, `SS.SCAN <container> <cursor> [MATCH p] [COUNT n]` on **any** container type |
| Type ops | `SS.OP <container> <op> <canonical-args>` generic; generated verbs `SS.<TYPE>.<OP>` e.g. `SS.VEC.SEARCH vectors '<canonical op-args>'` (aliases from `Datatype::operations()`) |
| Options | `SS.OPTIONS SET quorum <L> [timeout <ms>]`, `SS.OPTIONS GET`, `SS.WITH quorum <L> timeout <ms> <cmd …>`, `SS.LAST` (options applied to the previous command) |
| Server commands | `PING ECHO QUIT HELLO INFO [section] COMMAND [COUNT|INFO|DOCS] CONFIG GET (read-only subset) TIME DBSIZE FLUSHDB` (`FLUSHALL` → same as `FLUSHDB` for the target container); `INFO` reports `redis_version:7.2.0-spacestorage`, `spacestorage_namespace`, `spacestorage_container` |
| Not supported (v1) | lists, sets, sorted sets, streams, geo, bitmaps, HyperLogLog, pub/sub, Lua, transactions (`MULTI/EXEC` → `-ERR unsupported: MULTI`), `WATCH`, keyspace notifications, replication commands → `-ERR unsupported: <command>` (these families map to L2 types arriving with `03`) |
| Error forms | `-ERR`, `-NOAUTH`, `-WRONGPASS`, `-WRONGTYPE Operation against a key holding the wrong kind of value` (type mismatch), `-TIMEOUT applied=<ms> source=<src>`, `-UNAVAILABLE requested=<L> available=<n>`, `-ERR invalid option value: <option>=<value>` |
| Name escaping (FR-019) | none needed (bulk strings are binary-safe) |
| Stock clients (SC-001) | `redis-cli`, `redis` (Rust), `redis-py`, `ioredis`, Jedis/Lettuce, `go-redis` |
| Metrics | protocol label `redis`; `kind` = command name lower-case |
