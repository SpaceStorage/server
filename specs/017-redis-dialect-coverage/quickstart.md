# Quickstart: Complete-Product Redis Dialect Coverage

**Feature**: `017-redis-dialect-coverage` | **Spec**: [spec.md](spec.md)

Runnable validation guide for the complete-product Redis dialect. Tests assume a running `spacestoraged` node with the `CompleteProduct` dialect profile.

## Prerequisites

1. Build the workspace:
```bash
cargo build --release
```

2. Start the server with complete-product config:
```bash
./target/release/spacestoraged --config contracts/fixtures/complete-product-redis.conf
```

3. Have `redis-cli` installed (from Homebrew, apt, etc.) or use any Redis client library.

---

## Step 1: Connection and Auth

Test basic connection and `AUTH`:

```bash
# Connect to the Redis protocol port
redis-cli -p 6379

# Expected: OK (if credentials configured) or NOPERM error
AUTH mypassword

# Test PING
PING
# Expected: PONG

PING "hello"
# Expected: "hello"
```

Verify `SELECT` behavior:
```bash
SELECT 0
# Expected: OK (no-op, namespace already bound)

SELECT 1
# Expected: (error) ERR SELECT is not supported: namespaces are bound to credentials
```

---

## Step 2: Strings and Basic Operations

Test string operations:

```bash
SET mykey "hello world"
# Expected: OK

GET mykey
# Expected: "hello world"

EXISTS mykey
# Expected: (integer) 1

TYPE mykey
# Expected: string

TTL mykey
# Expected: (integer) -1 (no expiry)

EXPIRE mykey 60
# Expected: (integer) 1

TTL mykey
# Expected: (integer) ~60 (counting down)

DEL mykey
# Expected: (integer) 1

GET mykey
# Expected: (nil)
```

Test `MGET`/`MSET`:
```bash
MSET a 1 b 2 c 3
# Expected: OK

MGET a b c nonexistent
# Expected: 1) "1" 2) "2" 3) "3" 4) (nil)
```

---

## Step 3: Collection Types

### Hash

```bash
HSET user:1000 name "Alice" age "30"
# Expected: (integer) 2

HGET user:1000 name
# Expected: "Alice"

HGETALL user:1000
# Expected: 1) "name" 2) "Alice" 3) "age" 4) "30"  (field/value pairs in unspecified order)

HLEN user:1000
# Expected: (integer) 2

TYPE user:1000
# Expected: hash
```

Test `WRONGTYPE`:
```bash
SET mystring "hi"
HGET mystring field
# Expected: (error) WRONGTYPE Operation against a key holding the wrong kind of value
```

### List

```bash
LPUSH mylist "first" "second"
# Expected: (integer) 2

RPUSH mylist "third"
# Expected: (integer) 3

LRANGE mylist 0 -1
# Expected: 1) "second" 2) "first" 3) "third"

LPOP mylist
# Expected: "second"

LLEN mylist
# Expected: (integer) 2
```

### Set

```bash
SADD myset "a" "b" "c"
# Expected: (integer) 3

SMEMBERS myset
# Expected: 1) "a" 2) "b" 3) "c"

SISMEMBER myset "b"
# Expected: (integer) 1

SISMEMBER myset "z"
# Expected: (integer) 0
```

### Sorted Set

```bash
ZADD scores 100 "alice" 200 "bob" 150 "carol"
# Expected: (integer) 3

ZRANK scores "bob"
# Expected: (integer) 2 (0-indexed rank)

ZRANGE scores 0 -1 WITHSCORES
# Expected: 1) "alice" 2) "100" 3) "carol" 4) "150" 5) "bob" 6) "200"

ZSCORE scores "carol"
# Expected: "150"
```

---

## Step 4: Streams and Consumer Groups

### Basic Stream Operations

```bash
XADD mystream * sensor-id 1234 temperature 19.5
# Expected: (entry ID like "1634567890123-0")

XADD mystream * sensor-id 1235 temperature 20.1
# Expected: (next entry ID)

XLEN mystream
# Expected: (integer) 2

XRANGE mystream - +
# Expected: Two entries with fields
```

### Consumer Groups

```bash
XGROUP CREATE mystream mygroup 0 MKSTREAM
# Expected: OK (create at `0`, not `$`, so the two existing entries are deliverable to `XREADGROUP ... >`)

XREADGROUP GROUP mygroup consumer1 COUNT 1 STREAMS mystream >
# Expected: One pending entry

XPENDING mystream mygroup
# Expected: Summary of pending entries

XACK mystream mygroup <entry-id>
# Expected: (integer) 1
```

---

## Step 5: Blocking Commands

Test blocking with short timeout:

```bash
# Terminal 1: Block waiting for list pop
BLPOP waitlist 5
# This will block for 5 seconds or until item pushed

# Terminal 2: Push item
LPUSH waitlist "released"
# Terminal 1 should immediately return: 1) "waitlist" 2) "released"
```

Test timeout:
```bash
BLPOP emptylist 1
# Expected: (nil) after 1 second
```

---

## Step 6: Transactions

```bash
MULTI
# Expected: OK

SET txkey "value"
# Expected: QUEUED

INCR txcounter
# Expected: QUEUED

EXEC
# Expected: 1) OK 2) (integer) 1
```

Test `WATCH`:
```bash
SET watchedkey 1

WATCH watchedkey
# Expected: OK

MULTI
INCR watchedkey
EXEC
# Expected: 1) (integer) 2

# Now test with concurrent modification:
WATCH watchedkey
# (another client modifies watchedkey here)
MULTI
INCR watchedkey
EXEC
# Expected: (nil) - transaction aborted due to WATCH
```

---

## Step 7: Pub/Sub

Terminal 1 (subscriber):
```bash
SUBSCRIBE news
# Expected: Enter subscribe mode, shows: 1) "subscribe" 2) "news" 3) (integer) 1
```

Terminal 2 (publisher):
```bash
PUBLISH news "hello subscribers"
# Expected: (integer) 1 (number of receivers)
# Terminal 1 should show: 1) "message" 2) "news" 3) "hello subscribers"
```

Terminal 1 unsubscribe:
```bash
UNSUBSCRIBE news
# Expected: Returns to normal command mode
```

---

## Step 8: HyperLogLog

```bash
PFADD visitors user1 user2 user3
# Expected: (integer) 1

PFCOUNT visitors
# Expected: (integer) 3 (approximate)

PFADD visitors user2 user4
# Expected: (integer) 1 (new element user4 added)

PFCOUNT visitors
# Expected: (integer) 4 (approximate)

# Create another HLL
PFADD more_visitors user5 user6

PFMERGE all_visitors visitors more_visitors
# Expected: OK

PFCOUNT all_visitors
# Expected: (integer) 6 (approximate union)

# Verify Redis reports type as string
TYPE visitors
# Expected: string
```

---

## Step 9: Bitmaps

```bash
SETBIT mybitmap 100 1
# Expected: (integer) 0 (previous value)

GETBIT mybitmap 100
# Expected: (integer) 1

GETBIT mybitmap 99
# Expected: (integer) 0

BITCOUNT mybitmap
# Expected: (integer) 1

SETBIT mybitmap 200 1
BITCOUNT mybitmap
# Expected: (integer) 2

# BITOP example
SETBIT bm1 0 1
SETBIT bm1 1 1
SETBIT bm2 1 1
SETBIT bm2 2 1

BITOP AND result bm1 bm2
BITCOUNT result
# Expected: (integer) 1 (only bit 1 was set in both)

TYPE mybitmap
# Expected: string
```

---

## Step 10: INFO Command

```bash
INFO server
# Expected: Server version info, etc.

INFO keyspace
# Expected: Keys count, expires, etc. for this namespace

DBSIZE
# Expected: (integer) N (approximate count of keys)
```

---

## Step 11: Not-Supported Commands (Error Testing)

Verify the not-supported list returns correct errors:

```bash
GEOADD mygeo 13.361389 38.115556 "Palermo"
# Expected: (error) ERR unknown command 'GEOADD'

EVAL "return redis.call('GET', KEYS[1])" 1 mykey
# Expected: (error) ERR unknown command 'EVAL'

CLUSTER NODES
# Expected: Either info showing cluster disabled, or ERR unknown command

ACL LIST
# Expected: (error) ERR unknown command 'ACL'
```

---

## Conformance Test

Run the automated conformance suite:

```bash
# Run all Redis dialect tests
cargo test -p spacestorage-conformance --features redis-dialect

# Run specific test categories
cargo test -p spacestorage-conformance --features redis-dialect -- strings
cargo test -p spacestorage-conformance --features redis-dialect -- collections
cargo test -p spacestorage-conformance --features redis-dialect -- streams
cargo test -p spacestorage-conformance --features redis-dialect -- transactions
cargo test -p spacestorage-conformance --features redis-dialect -- pubsub
```

---

## Cross-Protocol Test

Verify Redis keys are visible via other protocols:

1. Write via Redis:
```bash
redis-cli -p 6379 SET shared "test value"
```

2. Read via PostgreSQL protocol (if configured):
```bash
psql -h localhost -p 5432 -c "SELECT * FROM kv_collection WHERE key = 'shared';"
```

---

## Troubleshooting

| Issue | Resolution |
|-------|------------|
| `ERR SELECT is not supported` | Only `SELECT 0` works; others are rejected per `002` |
| `WRONGTYPE` on GET | Key is not a string; use `TYPE` to check actual type |
| `EXEC` returns `nil` | `WATCH` detected key change; retry transaction |
| Pub/sub messages not received | Ensure same namespace; pub/sub is namespace-scoped |
| Blocking command returns nil | Timeout reached before condition met |
| `ERR unknown command` | Command is in not-supported list (geospatial, Lua, etc.) |

---

## Next Steps

After confirming all steps above pass:
1. Run full conformance suite: `cargo test -p spacestorage-conformance`
2. Verify first-binary profile still passes `016` tests
3. Review and approve the dialect specification
