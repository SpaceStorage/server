# Protocol Contract: `cassandra`

**Crate**: `crates/handler-cassandra` | **Stack**: `cassandra-protocol` (frame codecs) + hand-written CQL parser (`src/cql/`) | Research: R3

| Item | Value |
|------|-------|
| Wire version | CQL native protocol **v4** and **v5** (negotiated via `STARTUP`; unsupported version → `ERROR PROTOCOL_ERROR` listing supported); v5 framing (segments, CRC24/CRC32) supported; compression `lz4` (`lz4_flex`) and `none`; `snappy` refused in `SUPPORTED` |
| Signature (FR-004) | first byte `0x04` or `0x05` (version, request direction), byte 4 opcode ∈ `{0x05 OPTIONS, 0x01 STARTUP}`; body length ≤ 256 MiB; otherwise `ERROR PROTOCOL_ERROR` and close |
| Authentication | `AUTHENTICATE org.apache.cassandra.auth.PasswordAuthenticator` → SASL PLAIN (`\0user\0pass`) → `Authenticator::Password`; `AUTH_SUCCESS`/`AUTH_ERROR` |
| Namespace / schema | keyspace = namespace (`USE ks;` or fully-qualified `ks.table`); tables live in schema `public`; `CREATE KEYSPACE` creates the namespace implicitly (replication options accepted and ignored until `04`) |
| Session identity | `STARTUP` options `APPLICATION_NAME`, `DRIVER_NAME/VERSION` → `client_app` |
| Type option | `CREATE TABLE ks.t (…) WITH spacestorage_type = '<type>' AND spacestorage_opt_<name> = '<value>'`; default `relational_table` |
| Listing (FR-013) | `system_schema.keyspaces`, `system_schema.tables` (+ `extensions['spacestorage_type']` and `comment = 'spacestorage_type=<type>'`), `system_schema.columns`; `system.local`, `system.peers` (this node only until `06`), `system.spacestorage_containers` view with `(schema, name, type, created_via, created_at)` |
| Native mappings | `relational_table` ↔ CQL table (types `boolean, tinyint, smallint, int, bigint, float, double, decimal(text), text/varchar/ascii, blob, uuid/timeuuid, timestamp, date, time, list<T>, set<T>, map<K,V>, tuple`) with partition + clustering keys; `kv_collection` ↔ `(key text PRIMARY KEY, value text)`; `document_store` ↔ `(_id text PRIMARY KEY, doc text)` (canonical doc in `text`) |
| Canonical carrier | `text` (read), accepts `text`/`blob` |
| Type ops | CQL functions `spacestorage_op(container, op, args_text)` and generated `spacestorage_<op>(…)`: `SELECT * FROM spacestorage_knn('vectors', '[…]', 10);` |
| Options | consistency level flag (native); `USING TIMEOUT 5s` clause (native); custom payload keys `spacestorage.quorum`, `spacestorage.timeout` (per frame = per query; on `STARTUP`/`REGISTER` = session); inspection `SELECT * FROM system.spacestorage_options`; `SERIAL`/`LOCAL_SERIAL` consistency accepted only with `IF` conditions and mapped to `QUORUM`/`LOCAL_QUORUM` (documented) |
| Statements (v1) | `CREATE/ALTER/DROP KEYSPACE|TABLE [IF [NOT] EXISTS]`, `USE`, `SELECT` (`WHERE` on key columns and `ALLOW FILTERING` on others, `ORDER BY` clustering, `LIMIT`, `PER PARTITION LIMIT`, `COUNT(*)`), `INSERT [IF NOT EXISTS] [USING TTL|TIMESTAMP|TIMEOUT]`, `UPDATE [IF …]`, `DELETE [IF EXISTS]`, `BATCH` (logged/unlogged → `Batch`), `TRUNCATE`, `PREPARE/EXECUTE` (prepared id = hash of normalised CQL), `REGISTER` (accepted; `SCHEMA_CHANGE` events emitted to registered connections, `TOPOLOGY_CHANGE/STATUS_CHANGE` never in v1) |
| Not supported (v1) | materialized views, secondary indexes (`CREATE INDEX` → no-op with warning), UDTs/UDFs/UDAs, counters (until `03`), `GRANT/REVOKE` (until `07`) → `INVALID` naming the construct |
| Error forms | `UNAVAILABLE {cl, required, alive}` for explicit unsatisfiable quorum; `READ_TIMEOUT`/`WRITE_TIMEOUT {cl, received, blockfor, …}` for timeouts; clamped defaults reported via `WARNING` (v4+ frame flag) on the result |
| Name escaping (FR-019) | CQL identifiers are case-folded unless quoted; container names needing case or special characters are exposed quoted; names > 48 chars are exposed truncated with `~<hash8>` and listed in full in `system.spacestorage_containers` |
| Stock clients (SC-001) | `cqlsh` (5.x/6.x), DataStax Java driver 4.x, `scylla` Rust driver, `cassandra-driver` (Python), `gocql` |
| Metrics | protocol label `cassandra`; `kind` from statement type |
