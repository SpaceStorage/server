# Quickstart: shared planner (first binary) and optional distributed engines

**Feature**: `005-query-execution`

Proves SC-001, SC-003, SC-004, SC-006 (SERIALIZABLE refuse), SC-007, SC-012 on a first-binary node; SC-002 / SC-011 / SC-005 need the `016` three-node fixtures; SC-008 / SC-009 / SC-010 need complete-product / `query-distributed`.

Prerequisites, keys, and one-node/three-node start: [016 quickstart](../../016-mvp-and-nongoals/quickstart.md). Build default features (PostgreSQL + Redis only). Implementation of slices 1–5 plus this planner must exist before commands succeed.

## 0. Config validation

```bash
spacestorage validate specs/005-query-execution/contracts/fixtures/invalid/query-max-concurrent-zero.conf
# expected: exit 2, code query_max_concurrent_zero

spacestorage validate specs/005-query-execution/contracts/fixtures/invalid/query-spill-unknown.conf
# expected: exit 2, code query_spill_unknown
```

Append [query-block.conf](contracts/fixtures/query-block.conf) to the first-binary one-node conf (or rely on built-in defaults — they match).

## 1. Shared planner smoke (SC-001, SC-012)

With `spacestoraged` running on the one-node starter:

```bash
psql -h 127.0.0.1 -p 5432 -U demo -d acme -c 'CREATE TABLE t (k int PRIMARY KEY, v text);'
psql -h 127.0.0.1 -p 5432 -U demo -d acme -c "INSERT INTO t VALUES (1, 'hello');"
psql -h 127.0.0.1 -p 5432 -U demo -d acme -c 'SELECT * FROM t;'          # 1 | hello
psql -h 127.0.0.1 -p 5432 -U demo -d acme -c 'BEGIN;'                    # ERROR feature_not_supported (first binary)
psql -h 127.0.0.1 -p 5432 -U demo -d acme -c 'COPY t FROM STDIN;'        # ERROR feature_not_supported

redis-cli -h 127.0.0.1 -p 6379 --user demo --pass … SET k1 hello
redis-cli -h 127.0.0.1 -p 6379 GET k1
```

```bash
spacestorage executions --limit 5 --output json | jq '.[0].engine'
# expected: "planner"  (not a protocol name, not "local")
```

Prepared: `PREPARE q AS SELECT * FROM t WHERE k = $1; EXECUTE q(1);` must match the ad-hoc SELECT (FR-023).

## 2. Timeout and cancel (SC-003, SC-004)

```bash
# session timeout 1s; a query that cannot finish (e.g. wait hook or huge scan under test)
psql -h 127.0.0.1 -p 5432 -U demo -d acme -c "SET spacestorage.timeout = '1s'; SELECT pg_sleep(30);"
# expected: protocol timeout error within 2s of start; executions show outcome=timeout
```

Disconnect mid-query (close the client) → record `cancelled`. Explicit cancel uses the protocol cancel message or `POST /v1/jobs/{id}/cancel` for async jobs.

## 3. Isolation refuse (SC-006)

```bash
psql -h 127.0.0.1 -p 5432 -U demo -d acme -c 'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE;'
# expected: not-supported naming READ COMMITTED and SNAPSHOT — even if BEGIN is also not-supported in first binary
```

Complete-product: `BEGIN` + `COMMIT`/`ROLLBACK` visibility as in [isolation-and-txns.md](contracts/isolation-and-txns.md).

## 4. Admission (SC-007)

Set `query { max_concurrent_per_node 1; }` on a test node. Open one long query, submit a second:

```text
# expected: admission_rejected{limit:node, current:1, max:1}; 0 hang
```

## 5. Three-node: unavailability, partial option, and forward (SC-005, SC-011)

Use `016` three-node starters. Stop all replicas of one shard (or the only replica of a key).

```bash
psql -h 127.0.0.1 -p 5432 -U demo -d acme -c 'SELECT * FROM t WHERE k = 1;'
# expected: part_unavailable; not empty success; query fails as a whole (partial default off)

psql -h 127.0.0.1 -p 5432 -U demo -d acme -c "SET spacestorage.partial = on; SELECT * FROM t;"
# expected: reachable rows plus named missing part; not reported as complete
```

Write to a coordinator in a follower domain (two-domain fixture from `004`) → forwarded; `LOCAL_ONE` write still durable in source.

## 6. EXPLAIN (SC-008, complete product or first binary MAY)

```bash
psql -h 127.0.0.1 -p 5432 -U demo -d acme -c 'EXPLAIN SELECT * FROM t;'
# expected: logical plan names container t and Scan; executions show stage stopped at Planned
```

## 7. Join / subscribe (SC-009, SC-010) — `--features query-distributed`

Two-node cluster, two tables, `SELECT … JOIN`. Then wait **without** keeping the original request open — SQL poll, not `LISTEN`:

```bash
psql -h 127.0.0.1 -p 5432 -U demo -d acme -c "SET spacestorage.async = on; SELECT count(*) FROM a JOIN b ON a.k = b.k;"
# expected: one-row job id; original statement does not stream the join

psql -h 127.0.0.1 -p 5432 -U demo -d acme -c "SELECT spacestorage.job_wait('«id»');"
# expected: Succeeded (same as:)

spacestorage jobs wait «id»
```

```bash
psql -h 127.0.0.1 -p 5432 -U demo -d acme -c 'LISTEN jobs;'
# expected: not-supported (015); job_wait is the SQL path
```

Shuffle peer kill → named `StageFailed` or retry within timeout; never hang.

Automated equivalent: `cargo test -p spacestorage-conformance --features first-binary planner_` and, later, `--features query-distributed`.
