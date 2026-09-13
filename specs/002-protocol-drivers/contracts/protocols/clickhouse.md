# Protocol Contract: `clickhouse` and `clickhouse-http`

**Crate**: `crates/handler-clickhouse` (one driver, two handlers) | **Stack**: hand-written native protocol, `lz4_flex`, in-house `cityhash102`, `axum`/`hyper` for HTTP, `sqlparser` (`ClickHouseDialect`) | Research: R6; Clarification Q5

Both handlers share SQL parsing, type mappings, options handling, namespace rule and the block encoder; they differ only in transport.

## Common

| Item | Value |
|------|-------|
| Namespace / schema | `database` = namespace (native `Hello`/`Query` database field, HTTP `?database=` or `X-ClickHouse-Database`, `USE db`); must equal principal's namespace unless role `admin`; tables live in schema `public` (ClickHouse has no schema level) |
| Type option | `CREATE TABLE t (…) ENGINE = SpaceStorage('<type>'[, <opt>=<value> …])`; `ENGINE = MergeTree/ReplacingMergeTree/Memory/Log/TinyLog/StripeLog` (with `ORDER BY`/`PRIMARY KEY`) accepted as `relational_table` with the ordering key as primary key; default `relational_table` |
| Listing (FR-013) | `SHOW TABLES [FROM db]`, `SHOW DATABASES`, `system.tables` (`engine = 'SpaceStorage(<type>)'`, `engine_full`, `create_table_query`), `system.columns`, `system.databases`, `SHOW CREATE TABLE`, `DESCRIBE TABLE`, `EXISTS TABLE`, `system.spacestorage_containers` |
| Native mappings | `relational_table` ↔ table with column types `UInt8..64 Int8..64 Float32/64 Decimal(P,S)(text) String FixedString(N) Date Date32 DateTime DateTime64(P) UUID Bool Nullable(T) Array(T) Map(K,V) LowCardinality(T) Enum8/16(text)`; `kv_collection` ↔ `(key String, value String)`; `document_store` ↔ `(_id String, doc String)` (canonical doc; `JSON` type when client requests) |
| Canonical carrier | `String` column; `FORMAT JSONEachRow` field; `INSERT INTO vectors FORMAT JSONEachRow {"$d":…}` |
| Type ops | table functions generated from `Datatype::operations()`: `SELECT * FROM spacestorageOp('vectors', 'knn', '<canonical op-args>')`, `spacestorageKnn('vectors', [0.1,…], 10)` |
| Options | settings `spacestorage_quorum` (String), `spacestorage_timeout` (UInt64 ms) via `SET`, `SETTINGS` clause, HTTP params; native `max_execution_time` (seconds) → timeout; inspection `system.spacestorage_options`, HTTP response headers `X-SpaceStorage-Applied-*` |
| Statements (v1) | `CREATE/DROP/ALTER TABLE [IF [NOT] EXISTS]` (ALTER: `ADD/DROP/MODIFY COLUMN`, `MODIFY SETTING`), `CREATE/DROP DATABASE`, `INSERT … VALUES`, `INSERT … FORMAT <fmt>` (data in following blocks / body), `INSERT … SELECT`, `SELECT` (projection incl. aliases and scalar functions `lower upper length toString toInt* toFloat* now today concat coalesce if`, `WHERE`, `GROUP BY` + `count sum min max avg uniq(exact) any`, `ORDER BY`, `LIMIT [OFFSET]`, `LIMIT n BY`, `PREWHERE` (= WHERE), 2-way `JOIN`, `FINAL` ignored, `FORMAT`, `SETTINGS`), `ALTER TABLE … DELETE|UPDATE WHERE` (mutations, synchronous), `TRUNCATE`, `SHOW/DESCRIBE/EXISTS`, `SET`, `USE`, `SELECT version()`, `SELECT 1`, `SYSTEM FLUSH LOGS` (no-op), `KILL QUERY` (cancels by `query_id`) |
| Not supported (v1) | materialized views, dictionaries, distributed/replicated engines (accepted names are mapped to `relational_table` with a `NOTICE`-style log), window functions, `ARRAY JOIN`, `WITH` CTEs, `GLOBAL IN`, projections, TTL clauses, `OPTIMIZE` (no-op) → exception code 48 `NOT_IMPLEMENTED` naming the construct |
| Error forms | `Exception { code, name, message, stack_trace: "", nested }`; codes: 60 `UNKNOWN_TABLE`, 57 `TABLE_ALREADY_EXISTS`, 81 `UNKNOWN_DATABASE`, 47 `UNKNOWN_IDENTIFIER`, 62 `SYNTAX_ERROR`, 53 `TYPE_MISMATCH`, 36 `BAD_ARGUMENTS`, 48 `NOT_IMPLEMENTED`, 159 `TIMEOUT_EXCEEDED`, 285 `TOO_FEW_LIVE_REPLICAS`, 516 `AUTHENTICATION_FAILED`, 497 `ACCESS_DENIED`, 101 `UNEXPECTED_PACKET_FROM_CLIENT` |
| Name escaping (FR-019) | identifiers back-quoted when needed; otherwise none |
| Metrics | protocol label `clickhouse` / `clickhouse-http`; `kind` from statement type |

## `clickhouse` (native TCP)

| Item | Value |
|------|-------|
| Wire version | native protocol, server revision **54460** announced; accepts client revisions ≥ 54406 (feature flags gated by min(client, server) revision as ClickHouse does); packets: client `Hello(0) Query(1) Data(2) Cancel(3) Ping(4) TablesStatusRequest(5) KeepAlive(6) Scalar(7) IgnoredPartUUIDs(8) ReadTaskResponse(9) MergeTreeReadTaskResponse(10)` (5–10 accepted and ignored/empty), server `Hello(0) Data(1) Exception(2) Progress(3) Pong(4) EndOfStream(5) ProfileInfo(6) Totals(7) Extremes(8) TablesStatusResponse(9) Log(10) TableColumns(11) …`; `Addendum` (quota key) after `Hello` when revision ≥ 54458 |
| Signature (FR-004) | first varint `0` (`Hello`) followed by a length-prefixed client name string ≤ 256 bytes within 1 s; otherwise `Exception 101 UNEXPECTED_PACKET_FROM_CLIENT` and close |
| Authentication | `Hello` fields `user`, `password` (plain) → `Authenticator::Password`; `Hello` `database` → namespace; SSL via entrypoint TLS (`--secure` clients) |
| Compression | `Query.compression = 1` → blocks compressed with method byte `0x82` LZ4 and CityHash128 (v1.0.2) checksum over `(method, compressed_size, decompressed_size, data)`; checksum mismatch → `Exception 40 CHECKSUM_DOESNT_MATCH` |
| Data blocks | `BlockInfo` (is_overflows, bucket_num), columns as `(name, type string, data)` with per-type binary encodings; `Nullable` null-map prefix; `Array` offsets; `LowCardinality` dictionary encoding read, written as plain `String`; `INSERT … FORMAT Native` receives client blocks; `SELECT` results streamed in blocks of ≤ 65 505 rows with `Progress` and final `ProfileInfo`, `EndOfStream` |
| Session | settings set via `SET` persist for the connection; `Query.settings` block (string-typed settings with flags) parsed; `query_id` used for `KILL QUERY` and `X-ClickHouse-Query-Id` parity; `client_info` (name, version, `os_user`, `client_hostname`) → `client_app` |
| Stock clients (SC-001) | `clickhouse-client`, `clickhouse` (Rust, native), `clickhouse-driver` (Python, native), `clickhouse-go` v2 (native), JDBC (native mode) |

## `clickhouse-http`

| Item | Value |
|------|-------|
| Wire version | HTTP/1.1 (keep-alive); `GET /?query=` and `POST /` (query in body or `?query=` with data in body), `POST /?query=INSERT … FORMAT X` with body data; endpoints `/ping` (200 `Ok.`), `/replicas_status` (200 `Ok.`), `/` with no query → `200 Ok.`; response headers `X-ClickHouse-Server-Display-Name`, `X-ClickHouse-Query-Id`, `X-ClickHouse-Format`, `X-ClickHouse-Timezone`, `X-ClickHouse-Summary` (JSON: read_rows, read_bytes, written_rows, written_bytes, result_rows) |
| Signature (FR-004) | HTTP request line within 1 s; otherwise `400` |
| Authentication | Basic auth, or `X-ClickHouse-User`/`X-ClickHouse-Key` headers, or `?user=&password=` params → `Authenticator::Password`; failure → `403` with `X-ClickHouse-Exception-Code: 516` and CH-style text body (ClickHouse convention) |
| Formats | `TabSeparated`, `TabSeparatedWithNames`, `TabSeparatedWithNamesAndTypes` (default), `TSV*` aliases, `CSV`, `CSVWithNames`, `JSON`, `JSONCompact`, `JSONEachRow`, `JSONCompactEachRow`, `JSONStrings`, `Values`, `RowBinary`, `RowBinaryWithNamesAndTypes`, `Native`, `Pretty`, `PrettyCompact`, `Null`; `?default_format=`, `FORMAT` clause, `X-ClickHouse-Format` header |
| Session | `?session_id=` + `?session_timeout=` (default 60 s) keeps `SET` settings across requests; `?database=`; `?query_id=`; `?max_execution_time=`; `?spacestorage_quorum=`/`?spacestorage_timeout=`; `?enable_http_compression=1` + `Accept-Encoding: gzip|deflate|lz4` via pure-Rust `flate2` (`miniz_oxide` backend) and `lz4_flex`; `zstd` and `br` are not offered in v1 (their crates are C-backed) |
| Errors | HTTP status + `X-ClickHouse-Exception-Code` + text body `Code: N. DB::Exception: message. (NAME)`; mapping per data-model §8 |
| Stock clients (SC-001) | `curl`, `clickhouse-connect` (Python), `@clickhouse/client` (Node), `clickhouse` (Rust, HTTP mode), Grafana ClickHouse data source, DBeaver (HTTP JDBC) |
