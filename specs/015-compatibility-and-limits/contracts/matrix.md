# Contract: MUST / MUST NOT matrix

**Feature**: `015-compatibility-and-limits` | Spec: FR-001–FR-005, FR-008, FR-014, FR-015 | Intent table with spec replacements

Verbs outside MUST are MUST NOT. Not-supported uses `002` renderer; never a silent empty success.

Profile columns: FB = FirstBinary, HC = HandlersComplete, CP = CompleteProduct. `—` = handler absent.

## PostgreSQL

| Verb | FB | HC | CP |
|------|----|----|-----|
| Simple + extended query, INSERT/SELECT/UPDATE/DELETE, CREATE/ALTER/DROP mapped to containers, prepared, database name as namespace, `search_path` | MUST | MUST | MUST |
| BEGIN/COMMIT/ROLLBACK | MUST NOT | MUST NOT | MUST |
| COPY text/csv/binary | MUST NOT | MUST NOT | MUST |
| Forward-only DECLARE/FETCH/CLOSE in a transaction | MUST NOT | MUST NOT | MUST |
| EXPLAIN (logical plan) | MAY | MAY | MUST |
| WITH HOLD, SCROLL, FETCH BACKWARD, portal after COMMIT | MUST NOT | MUST NOT | MUST NOT |
| PL/pgSQL, stored procedures, triggers, LISTEN/NOTIFY, replication protocol, FDW, extensions, COPY PROGRAM | MUST NOT | MUST NOT | MUST NOT |
| SERIALIZABLE | MUST NOT | MUST NOT | MUST NOT |

## Redis (RESP2)

Complete-product dialect surface is owned by `017` (capability table); `015` FR-016 pins that ownership, and this table is the merged matrix. First-binary (`016`) Redis stays the narrow `K/V Store` string list.

| Verb | FB | HC | CP |
|------|----|----|-----|
| AUTH, PING, GET, SET, DEL, EXISTS, SCAN, SELECT (no-op in bound ns), TTL mapped to container TTL | MUST on KV | MUST on KV | MUST |
| `017` capability table: introspection (`INFO`, `DBSIZE`, `TYPE`, `SCAN`, `OBJECT`), strings + expiry, hashes, lists, sets, sorted sets, Streams + consumer groups, bitmaps, HyperLogLog, pub/sub, transactions | not-supported off the `016` list | not-supported off the `016` list | MUST |
| Type-specific commands via `002` mapping | not required off KV | not required off KV | MUST where mapped |
| Geo family, `XAUTOCLAIM`, Lua/`EVAL`/functions, modules, `CLUSTER` slots, Sentinel roles, ACL-management | MUST NOT | MUST NOT | MUST NOT |
| RESP3 | MUST NOT | MUST NOT | MAY later (spec change) |

## Cassandra (native v4 and v5)

| Verb | FB | HC | CP |
|------|----|----|-----|
| CQL DML/DDL mapped, consistency level, prepared, keyspace as namespace | — | MUST | MUST |
| LWT as a second txn model, native MVs, Cassandra CDC | — | MUST NOT | MUST NOT |

## Elasticsearch

See [elasticsearch-search.md](elasticsearch-search.md). No handler in FB.

## ClickHouse (`clickhouse` and `clickhouse-http`, both required in HC/CP)

| Verb | FB | HC | CP |
|------|----|----|-----|
| CREATE TABLE, INSERT, SELECT WHERE, GROUP BY (GROUP BY needs slice 8 spill/agg) | — | MUST (CRUD/WHERE); GROUP BY not-supported until CP | MUST |
| Dictionaries, ClickHouse-native replication, ClickHouse MVs | — | MUST NOT | MUST NOT |

## S3 (SigV4)

| Verb | FB | HC | CP |
|------|----|----|-----|
| ListBuckets, CreateBucket, Put/Get/DeleteObject, multipart, ListObjectsV2 | — | MUST | MUST |
| Versioning, object lock, S3 replication, S3 Select, ACLs beyond `014` | — | MUST NOT | MUST NOT |

## WebDAV (RFC 4918 subset)

| Verb | FB | HC | CP |
|------|----|----|-----|
| PROPFIND, GET, PUT, DELETE, MKCOL, MOVE, COPY | — | MUST | MUST |
| LOCK/UNLOCK as a lock product, CalDAV/CardDAV | — | MUST NOT | MUST NOT |

Handshake mismatch (wrong protocol on a port): refuse within 1 s in that protocol’s form (`002`).
