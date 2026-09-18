# Quickstart: Compatibility matrix, limits, isolation, version window

**Feature**: `015-compatibility-and-limits` | **Gates**: SC-001–SC-005 | First binary except where noted slice 6 / 8 / mixed-version

Prerequisites: `001` runtime, `016` first-binary types, `002` PG+Redis, `005` planner, `014` bootstrap admin. Model: [data-model.md](data-model.md). Matrix: [contracts/matrix.md](contracts/matrix.md).

Config: [first-binary.conf](contracts/fixtures/first-binary.conf).

## 0. Config validation

```bash
spacestorage validate specs/015-compatibility-and-limits/contracts/fixtures/invalid/limits-zero.conf
# expected: exit 2, limits_zero{knob:"max_key"}

spacestorage validate specs/015-compatibility-and-limits/contracts/fixtures/invalid/product-version-zero.conf
# expected: exit 2, product_version_zero
```

An entrypoint `handler elasticsearch` on a first-binary profile → `unknown_handler` (`016`).

## 1. First-binary MUST / MUST NOT (SC-001, SC-002, SC-003)

Start [first-binary.conf](contracts/fixtures/first-binary.conf). Stock `psql`: INSERT/SELECT/UPDATE/DELETE, CREATE/DROP, prepared succeed. `COPY`, `BEGIN`, `DECLARE CURSOR` → not-supported. `SET TRANSACTION ISOLATION LEVEL SERIALIZABLE` → `serializable_nongoal`.

Stock `redis-cli`: AUTH, PING, GET/SET/DEL/EXISTS/SCAN, SELECT (no-op), TTL mapped succeed. `CLUSTER SLOTS` / `EVAL` → not-supported. Unbound `admin` AUTH refused (`014`).

## 2. Size, connection, admission reject (SC-004)

Write a value > `max_value` (default 16MiB) → `limit_exceeded{limit:"value",…}`. Open `max_connections_per_entrypoint`+1 sessions → connection refuse, no hang. Drive `query.max_concurrent_per_node`+1 queries → `admission_rejected`. A query working set > `max_memory` with `spill off` → reject naming `query_memory`. 0 hangs.

## 3. Same-version cluster (SC-005 first binary)

Bring up the three-node `016` starter. Every node reports `product_version 1`. Mixed N/N+1 fixtures are **not** required.

## 4. Slice 6 handlers (SC-001/002 HC)

With `handlers-complete`: Cassandra/ES/CH/S3/WebDAV MUST smokes. ES search `match`/`term` succeeds. ES ILM and aggregations → not-supported. ClickHouse dictionaries → not-supported. S3 versioning → not-supported. WebDAV LOCK → not-supported. Handshake mismatch < 1 s.

## 5. Slice 8 complete product (SC-001–SC-004 CP)

`BEGIN`/`COMMIT`, COPY text/csv/binary, forward-only `DECLARE`/`FETCH`/`CLOSE` succeed; `WITH HOLD` not-supported. `REPEATABLE READ` on a Relational Table snapshots; on a `K/V Store` → `snapshot_unsupported`. Sort/hash/agg over `max_memory` with `spill on` complete via spill. ES closed aggregations succeed.

## 6. Mixed N/N+1 (SC-005 complete product)

After format versions exist: cluster N and N+1 interoperate. N node create of an N+1 type → `type_too_new`. N+2 join → `product_version_window`. `spacestorage catalog diff --from N --to N+1` lists the delta (`003`).

## Out of this quickstart

Auth mechanisms (`014`), placement (`004`), full engine internals (`005` beyond isolation/admission), quota unit accounting (`007`), numeric default *tuning* beyond the documented table.
