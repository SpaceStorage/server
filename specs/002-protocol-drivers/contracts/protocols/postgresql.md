# Protocol Contract: `postgresql`

**Crate**: `crates/handler-postgresql` | **Stack**: `pgwire` (server framework, SCRAM-SHA-256) + `sqlparser` (`PostgreSqlDialect`) | Research: R2

| Item | Value |
|------|-------|
| Wire version | PostgreSQL frontend/backend protocol **3.0** (`196608`); `SSLRequest` honoured when the entrypoint declares TLS, answered `N` otherwise; `GSSENCRequest` → `N`; `CancelRequest` cancels the in-flight statement of the referenced backend |
| Signature (FR-004) | first 8 bytes: `int32 len (8..=10000)`, `int32 code ∈ {196608, 80877103, 80877102, 80877104}`; otherwise `ErrorResponse 08P01` and close |
| Authentication | `SCRAM-SHA-256` (default), `password` (cleartext, only over TLS) — credentials resolved by `Authenticator::Password/ScramSha256` |
| Namespace / schema | startup parameter `database` = namespace (must equal principal's namespace unless role `admin`); `search_path` / explicit `schema.table` = schema (default `public`) |
| Session identity | `application_name` → `client_app`; `server_version` parameter from `protocols.postgresql.server_version`; `client_encoding` UTF8 only |
| Type option | `CREATE TABLE t (…) WITH (spacestorage_type = '<type>', spacestorage_opt_<name> = '<value>' …)`; default `relational_table`; column list optional for non-relational types |
| Listing (FR-013) | `SELECT * FROM spacestorage.containers` → `(schema, name, type, created_via, created_at, options jsonb)`; also `information_schema.tables` (`table_type = 'BASE TABLE'`, plus column `spacestorage_type`) and `pg_catalog.pg_class`/`pg_namespace`/`pg_attribute`/`pg_type` minimum so `psql \d`, `\dt`, `\dn` work |
| Native mappings | `relational_table` ↔ table rows (column types: `bool, int2/4/8, float4/8, numeric(text), text, bytea, uuid, timestamptz, jsonb, text[]`) ; `document_store` ↔ table with columns `(_id text, doc jsonb)` and field-path filters via `doc->>'field'`; `kv_collection` ↔ `(key text, value text/jsonb)` |
| Canonical carrier | `jsonb` (read), accepts `jsonb/json/text/bytea` (write) — e.g. `INSERT INTO vectors VALUES ('{"$d":…}'::jsonb)` |
| Type ops | set-returning functions in schema `spacestorage`, generated from `Datatype::operations()`: `SELECT * FROM spacestorage.knn('vectors', '[0.1,0.2,…]'::jsonb, 10)`, `spacestorage.op('vectors', 'knn', '<canonical op-args>'::jsonb)` generic form |
| Options | `SET/SHOW spacestorage.quorum|timeout`, `statement_timeout`, hint comment `/*+ spacestorage: quorum=… timeout=… */`; inspection `SELECT * FROM spacestorage.options` |
| Statements (v1) | `CREATE/ALTER/DROP TABLE [IF [NOT] EXISTS]`, `CREATE/DROP SCHEMA`, `INSERT [ON CONFLICT DO NOTHING|UPDATE]`, `SELECT` (projection, `WHERE`, `ORDER BY`, `LIMIT/OFFSET`, `GROUP BY` + `COUNT/SUM/MIN/MAX/AVG`, 2-way `JOIN`), `UPDATE`, `DELETE`, `TRUNCATE`, `SET/SHOW/RESET`, `BEGIN/COMMIT/ROLLBACK` (single-statement atomicity; documented), `DISCARD ALL`, `EXPLAIN` (prints the `LogicalRequest`), `COPY … FROM STDIN` (text/csv) and `COPY … TO STDOUT`; simple and extended query protocol; prepared statements; portals with row limits |
| Not supported (v1) | multi-way joins, subqueries, window functions, CTEs, `LISTEN/NOTIFY`, `CREATE INDEX` (accepted as no-op with `NOTICE`), functions/triggers, `LOCK`, cursors beyond portals → `0A000 feature_not_supported` naming the construct |
| Error forms | see [data-model.md §8](../../data-model.md#8-per-protocol-error-form-errorrenderer); `NOTICE` carries clamped-option info after each statement when `clamped` is set |
| Name escaping (FR-019) | container names not valid as PG identifiers are exposed via quoted identifiers; names > 63 bytes are exposed truncated with `~<hash8>` suffix and listed with their full name in `spacestorage.containers.name` |
| Stock clients (SC-001) | `psql`, `tokio-postgres`/`rust-postgres`, `psycopg`, JDBC, `pgx`, `node-postgres` |
| Metrics | protocol label `postgresql`; `kind` from statement type |
