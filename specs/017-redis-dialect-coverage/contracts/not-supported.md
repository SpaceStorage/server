# Contract: Not-Supported Commands

**Feature**: `017-redis-dialect-coverage` | Spec: FR-012

Closed list of commands and families that return the Redis not-supported error form. This is the complete list of what is explicitly out of scope for the complete-product dialect.

## Error Form

All not-supported commands return a Redis error and nothing else — no partial execution, no `+OK`, no empty-array success. Unless a command documents a more specific `002` form (the `SELECT n>0` entry in §11), the reply is:

```
-ERR unknown command '<command>'
```

Where `<command>` is the uppercase command name received. A bad subcommand of a listed command takes `-ERR unknown subcommand or wrong number of arguments`. The error is returned immediately.

## Not-Supported Families

### 1. Geospatial

Redis geospatial indices (based on sorted sets with geohash encoding).

| Command | Typical Redis Reply |
|---------|-------------------|
| `GEOADD key [NX|XX] [CH] longitude latitude member [member ...]` | -ERR unknown command 'GEOADD' |
| `GEOPOS key member [member ...]` | -ERR unknown command 'GEOPOS' |
| `GEODIST key member1 member2 [M|KM|FT|MI]` | -ERR unknown command 'GEODIST' |
| `GEORADIUS key longitude radius unit [WITHCOORD] [WITHDIST] [WITHHASH] [COUNT count] [ASC|DESC] [STORE key] [STOREDIST key]` | -ERR unknown command 'GEORADIUS' |
| `GEORADIUS_RO key longitude radius unit [...]` | -ERR unknown command 'GEORADIUS_RO' |
| `GEORADIUSBYMEMBER key member radius unit [...]` | -ERR unknown command 'GEORADIUSBYMEMBER' |
| `GEORADIUSBYMEMBER_RO key member radius unit [...]` | -ERR unknown command 'GEORADIUSBYMEMBER_RO' |
| `GEOSEARCH key [FROMMEMBER member] [FROMLONLAT lon lat] [BYRADIUS rad unit] [BYBOX w h unit] [ASC|DESC] [COUNT count] [WITHCOORD] [WITHDIST] [WITHHASH]` | -ERR unknown command 'GEOSEARCH' |
| `GEOSEARCHSTORE dest source [...]` | -ERR unknown command 'GEOSEARCHSTORE' |
| `GEOHASH key member [member ...]` | -ERR unknown command 'GEOHASH' |

**Rationale**: Requires geospatial indexing library or complex spatial data structures. Can be added later if required; for now, clients should store coordinates as regular sorted set members with manual distance calculations.

---

### 2. Stream Auto-Claim

| Command | Error |
|---------|-------|
| `XAUTOCLAIM key group consumer min-idle-time start [COUNT count] [JUSTID]` | -ERR unknown command 'XAUTOCLAIM' |

**Rationale**: Per spec FR-012. `XCLAIM` (manual claim) is supported; `XAUTOCLAIM` (automatic claiming based on idle time) is not. Clients can implement auto-claim logic using `XPENDING`, `XCLAIM`, and `XREADGROUP` Idle detection.

---

### 3. Lua Scripting / EVAL

| Command | Error |
|---------|-------|
| `EVAL script numkeys [key ...] [arg ...]` | -ERR unknown command 'EVAL' |
| `EVALSHA sha1 numkeys [key ...] [arg ...]` | -ERR unknown command 'EVALSHA' |
| `SCRIPT LOAD script` | -ERR unknown command 'SCRIPT' |
| `SCRIPT EXISTS sha1 [sha1 ...]` | -ERR unknown command 'SCRIPT' |
| `SCRIPT FLUSH` | -ERR unknown command 'SCRIPT' |
| `SCRIPT KILL` | -ERR unknown command 'SCRIPT' |
| `SCRIPT DEBUG` | -ERR unknown command 'SCRIPT' |

**Rationale**: Lua runtime violates Constitution I (Rust-only stack). No embedded scripting language; all logic must be in the handler or client-side.

---

### 4. Functions (Redis 7.0+)

| Command | Error |
|---------|-------|
| `FCALL function_name [numkeys [key ...] [arg ...]]` | -ERR unknown command 'FCALL' |
| `FCALL_RO function_name [numkeys [key ...] [arg ...]]` | -ERR unknown command 'FCALL_RO' |
| `FUNCTION LOAD [REPLACE] [LIBRARYNAME libname] "{engine_name}!{library_code}"` | -ERR unknown command 'FUNCTION' |
| `FUNCTION DELETE library_name` | -ERR unknown command 'FUNCTION' |
| `FUNCTION KILL` | -ERR unknown command 'FUNCTION' |
| `FUNCTION FLUSH` | -ERR unknown command 'FUNCTION' |
| `FUNCTION LIST [LIBRARYNAME libname] [WITHCODE]` | -ERR unknown command 'FUNCTION' |
| `FUNCTION STATS` | -ERR unknown command 'FUNCTION' |
| `FUNCTION DUMP` | -ERR unknown command 'FUNCTION' |
| `FUNCTION RESTORE payload [FLUSH|APPEND|REPLACE]` | -ERR unknown command 'FUNCTION' |

**Rationale**: Same as Lua; also requires a function runtime. Functions are a Redis 7.0 feature.

---

### 5. Modules

| Command | Error |
|---------|-------|
| `MODULE LOAD path [arg ...]` | -ERR unknown command 'MODULE' |
| `MODULE UNLOAD name` | -ERR unknown command 'MODULE' |
| `MODULE LIST` | -ERR unknown command 'MODULE' |
| `MODULE LOADEX path [CONFIG name value] [ARGS ...]` | -ERR unknown command 'MODULE' |

**Rationale**: Redis modules are native shared libraries loaded at runtime. Prohibited by Constitution I (Rust-only, no external modules loaded via C ABI).

---

### 6. Cluster (Redis Cluster protocol)

Full Redis Cluster protocol is NOT supported. However, harmless info commands reply with "disabled" rather than error:

| Command | Behavior |
|---------|----------|
| `CLUSTER INFO` | Returns static info indicating cluster mode disabled |
| `CLUSTER NODES` | Returns static single-node representation |
| `CLUSTER SLOTS` | -ERR unknown command 'CLUSTER' |
| `CLUSTER KEYSLOT key` | -ERR unknown command 'CLUSTER' |
| `CLUSTER ADDSLOTS slot [slot ...]` | -ERR unknown command 'CLUSTER' |
| `CLUSTER DELSLOTS slot [slot ...]` | -ERR unknown command 'CLUSTER' |
| `CLUSTER FLUSHSLOTS` | -ERR unknown command 'CLUSTER' |
| `CLUSTER SETSLOT slot IMPORTING|MIGRATING|NODE node-id` | -ERR unknown command 'CLUSTER' |
| `CLUSTER BUMPEPOCH` | -ERR unknown command 'CLUSTER' |
| `CLUSTER SAVECONFIG` | -ERR unknown command 'CLUSTER' |
| `CLUSTER FORGET node-id` | -ERR unknown command 'CLUSTER' |
| `CLUSTER REPLICATE node-id` | -ERR unknown command 'CLUSTER' |
| `CLUSTER SLAVES node-id` | -ERR unknown command 'CLUSTER' |
| `CLUSTER COUNT-FAILURE-REPORTS node-id` | -ERR unknown command 'CLUSTER' |
| `CLUSTER FAILOVER [FORCE|TAKEOVER]` | -ERR unknown command 'CLUSTER' |
| **... all other CLUSTER subcommands** | -ERR unknown command 'CLUSTER' |

**Rationale**: Cluster topology and slot migration is handled by SpaceStorage's `011` (membership) and `004` (placement), not Redis Cluster protocol. Scale-out uses SpaceStorage-native topology.

---

### 7. Sentinel

| Command | Error |
|---------|-------|
| `SENTINEL masters` | -ERR unknown command 'SENTINEL' |
| `SENTINEL master name` | -ERR unknown command 'SENTINEL' |
| `SENTINEL slaves name` | -ERR unknown command 'SENTINEL' |
| `SENTINEL sentinels name` | -ERR unknown command 'SENTINEL' |
| `SENTINEL get-master-addr-by-name name` | -ERR unknown command 'SENTINEL' |
| `SENTINEL reset pattern` | -ERR unknown command 'SENTINEL' |
| `SENTINEL failover name` | -ERR unknown command 'SENTINEL' |
| `SENTINEL ckquorum name` | -ERR unknown command 'SENTINEL' |
| `SENTINEL flushconfig` | -ERR unknown command 'SENTINEL' |
| **... all other SENTINEL subcommands** | -ERR unknown command 'SENTINEL' |

**Rationale**: Redis Sentinel is a separate high availability solution. SpaceStorage uses Raft for control plane (`006`) and has its own health checking; Sentinel protocol is not applicable.

---

### 8. ACL Management

| Command | Error |
|---------|-------|
| `ACL LIST` | -ERR unknown command 'ACL' |
| `ACL USERS` | -ERR unknown command 'ACL' |
| `ACL GETUSER username` | -ERR unknown command 'ACL' |
| `ACL SETUSER username [rules ...]` | -ERR unknown command 'ACL' |
| `ACL DELUSER username [username ...]` | -ERR unknown command 'ACL' |
| `ACL CAT [categoryname]` | -ERR unknown command 'ACL' |
| `ACL GENPASS [bits]` | -ERR unknown command 'ACL' |
| `ACL WHOAMI` | -ERR unknown command 'ACL' |
| `ACL LOG [count]` | -ERR unknown command 'ACL' |
| `ACL HELP` | -ERR unknown command 'ACL' |
| `ACL LOAD` | -ERR unknown command 'ACL' |
| `ACL SAVE` | -ERR unknown command 'ACL' |

**Rationale**: Authentication and authorization are handled by `002` (namespace/credential binding) and `014` (authz/keys). ACL file management is not supported; roles and permissions are managed through the SpaceStorage credential system.

**Note**: `AUTH` with username (ACL-style `AUTH username password`) is not-supported; only simple `AUTH password` (credential token) is supported per `002`.

---

### 9. Historical / Deprecated Commands

| Command | Error |
|---------|-------|
| `DEBUG OBJECT key` | -ERR unknown command 'DEBUG' |
| `DEBUG SEGFAULT` | -ERR unknown command 'DEBUG' |
| `DEBUG RELOAD` | -ERR unknown command 'DEBUG' |
| `DEBUG LOG` | -ERR unknown command 'DEBUG' |
| `DEBUG OOM` | -ERR unknown command 'DEBUG' |
| `SHUTDOWN [NOSAVE|SAVE]` | -ERR unknown command 'SHUTDOWN' |
| `SLAVEOF host port` | -ERR unknown command 'SLAVEOF' |
| `REPLICAOF host port` | -ERR unknown command 'REPLICAOF' |
| `SYNC` | -ERR unknown command 'SYNC' |
| `PSYNC` | -ERR unknown command 'PSYNC' |
| `MIGRATE host port key ...` | -ERR unknown command 'MIGRATE' |
| `RESTORE key ttl serialized-value` | -ERR unknown command 'RESTORE' |
| `DUMP key` | -ERR unknown command 'DUMP' |
| `RENAME key newkey` | -ERR unknown command 'RENAME' |
| `RENAMENX key newkey` | -ERR unknown command 'RENAMENX' |

**Rationale**: These relate to replication, persistence management, or admin operations that are handled differently in SpaceStorage. Replication is via `012` (internode) and `013` (durability). Admin operations use the `001` runtime or the UI.

---

### 10. RESP3 Only

| Command/Feature | Error |
|-----------------|-------|
| Hello 3 | -ERR unknown command 'HELLO' (or returns RESP2 only) |
| `CLIENT SETINFO` | -ERR unknown command 'CLIENT SETINFO' |
| Any RESP3-specific data type (big number, verbatim, etc.) | Returned as compatible RESP2 type |

**Rationale**: RESP3 is MAY-later per `015`; complete-product is RESP2 only for now.

---

### 11. Database Selection / Swap

The dialect binds a session to one namespace through its credential, so there is no second Redis database to select or swap (`002`). `SELECT 0` is a no-op `MUST` (the namespace is already bound); `SELECT n>0` and `SWAPDB` are not-supported.

| Command | Error |
|---------|-------|
| `SWAPDB index1 index2` | -ERR unknown command 'SWAPDB' |
| `SELECT index` for `index > 0` | -ERR SELECT is not supported: namespaces are bound to credentials |

**Rationale**: Namespaces replace numbered databases (`002`); the credential binds the namespace, so a non-zero `SELECT` has no target and `SWAPDB` has nothing to swap. `SELECT n>0` keeps the documented `002` form (it is an exception to the generic unknown-command form above); `SWAPDB` takes the standard form.

---

## Testing Contract

The conformance test `complete-product-not-supported-list` verifies that every command in this document returns the documented error form when invoked against a `CompleteProduct` dialect node.
