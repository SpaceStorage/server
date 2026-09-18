# Research: Protocol Compatibility Ceiling, Limits, Isolation, and Rolling Upgrade

**Feature**: `015-compatibility-and-limits` | **Date**: 2026-09-18

Each item resolves a Technical Context unknown or a technology choice. Format: Decision / Rationale / Alternatives considered. Inputs: [spec.md](spec.md) (clarify 2026-09-15/18), constitution 1.3.0, intent `15`, sibling plans `002` (handlers, error forms), `003` (catalog), `005` (admission, isolation, spill), `007` (quotas), `008` (metrics), `012` (internode N/N+1), `013` (format version), `014` (unbound admin), `016` (slices).

## R1. One crate for the ceiling

- **Decision**: Add `crates/compat` (`spacestorage-compat`). It owns `DialectProfile`, per-protocol MUST/MUST NOT tables, wire-version constants, `IsolationLevel` mapping, `Limits` defaults/types, and `ProductVersion` window predicates. Handlers call `classify` **before** building `LogicalRequest`. `exec` reads `Limits` + isolation. `internode`/`storage` call `compatible(n, peer)`.
- **Rationale**: Spec is the ceiling consumed by many crates; a table in one handler would drift.
- **Alternatives considered**: Grow `protocol-core` (rejected: internodes/storage should not depend on driver toolkit); copy lists into `002` and `005` (rejected: two sources); generate from intent markdown at build (fragile).

## R2. Dialect profiles, not a “v1” matrix

- **Decision**: `enum DialectProfile { FirstBinary, HandlersComplete, CompleteProduct }`. First binary = `016` slices 1–5 (PG auto-commit + Redis MUST on KV). `HandlersComplete` = slice 6 (all eight handlers at the matrix **except** slice-8 verbs: no SQL `BEGIN`/`COPY`/`DECLARE`, no ES aggregations, no SNAPSHOT). `CompleteProduct` = ceiling including slice 8. `release-profile` compiles FirstBinary; features `handlers-complete` and `query-distributed` widen the profile. Never name a profile “v1”.
- **Rationale**: Spec FR-004; `016` slices; ES aggregations wait for slice 8 (FR-015).
- **Alternatives considered**: One matrix with runtime feature flags per verb (noisier); calling first binary “v1” (clarify Q1).

## R3. Default numeric limits

- **Decision**: Documented defaults (operators MAY raise; zero is invalid):

  | Knob | Default | Enforced by |
  |------|---------|-------------|
  | `limits.max_key` | 1 KiB | handler at parse |
  | `limits.max_value` | 16 MiB | handler; S3 assembled object; multipart parts sum to this |
  | `limits.max_query_text` | 1 MiB | handler |
  | `limits.max_result` | 64 MiB | exec while producing |
  | `limits.max_connections_per_entrypoint` | 10 000 | node accept (`002` idle-session order) |
  | `limits.max_connections_per_principal` | 1 000 | after AUTH |
  | `query.max_concurrent_per_node` | 512 | `005` (unchanged knob) |
  | `query.max_concurrent_per_namespace` | 128 | `005`; `007` quota may lower |
  | `query.max_memory` | 256 MiB | `005` per-query working set |

- **Rationale**: Spec FR-011 existence is not a planning deferral of *behavior*; numbers were. Align concurrent/memory with existing `005` admission contract so we do not fork knobs. 16 MiB object + S3 multipart covers large objects without a 5 GiB single buffer.
- **Alternatives considered**: Redis-like 512 MiB values (OOM on first binary); unlimited keys (OOM); duplicating concurrent caps under `limits { }` (two knobs).

## R4. Spill policy vs `005` `query.spill off`

- **Decision**: First binary / `HandlersComplete`: `query.spill off` (MUST NOT require spill). `CompleteProduct` / slice 8: default `query.spill on`. Physical plans mark sort, hash join build, and aggregation as spillable; those MUST spill when over `max_memory`. Non-spillable plans (point get, insert) reject naming `query_memory`. Operator MAY set `spill off` on a complete-product node — then every over-memory query rejects (plan “cannot spill”). Concurrent-query overflow always rejects; never spills a slot.
- **Rationale**: Spec FR-010 / clarify Q1. Revises `005` complete-product default only.
- **Alternatives considered**: Always reject (rejected: ClickHouse GROUP BY MUST); spill in first binary (rejected: CRUD does not need temp files).

## R5. Isolation mapping and `snapshot_capable`

- **Decision**: `IsolationLevel::{ReadCommitted, Snapshot}`. PostgreSQL `SET TRANSACTION ISOLATION LEVEL REPEATABLE READ` and `SNAPSHOT` → `Snapshot`. Default / `READ COMMITTED` → `ReadCommitted`. `READ UNCOMMITTED` → `ReadCommitted` (documented upgrade, not a third level). `SERIALIZABLE` → not-supported (`0A000` / protocol equivalent) naming the allowed set. `003` `TypeDescriptor.snapshot_capable: bool`. First-binary types: `Relational Table` = true, `Document Store` = true, `K/V Store` = false. New types default **false** until the descriptor sets true. A SNAPSHOT txn that lists any non-capable container → refuse `snapshot_unsupported{type}`; no downgrade. First binary has no `BEGIN` so SNAPSHOT is not required there.
- **Rationale**: Spec FR-006 / clarify Q2.
- **Alternatives considered**: Silent downgrade (rejected); only Relational Table forever (rejected: Document Store is in the first-binary type set and can snapshot).

## R6. PostgreSQL SQL cursors vs extended portals

- **Decision**: First binary: extended-query named/unnamed **portals** stay as `002`/`016` (row-limited Execute). SQL `DECLARE`/`FETCH`/`CLOSE` → not-supported. Complete product: forward-only `DECLARE CURSOR [NO SCROLL] FOR …` inside an open transaction, `FETCH [FORWARD] [n|ALL]`, `CLOSE`. `WITH HOLD`, `SCROLL`, `FETCH BACKWARD`, using a cursor after `COMMIT` → not-supported. Implemented as a **held portal bound to the txn id**, dropped on COMMIT/ROLLBACK/disconnect.
- **Rationale**: Spec FR-014 / clarify Q3. Does not require holdable-across-commit storage.
- **Alternatives considered**: No SQL DECLARE ever (contradicts intent “holdable subset”); WITH HOLD MUST (a lock/product-like session store).

## R7. Elasticsearch search list is data here; `005` implements

- **Decision**: MUST search DSL: top-level `query` of `query_string` | `match` | `term` | `range` | `bool` (must/filter/should/must_not of those). Lowered to `LogicalRequest::Scan` + filter expr. MUST aggregations (slice 8): `terms`, `min`, `max`, `sum`, `avg`, `histogram`, `value_count` → `LogicalRequest::Aggregate`. ILM, ingest pipelines, ML, CCR, `script`, pipeline aggs, `significant_terms`, `composite`, etc. → not-supported **in the handler**, never a “best-effort” ES JSON. Slice 6 handler without slice 8: aggregations not-supported even if listed as complete-product MUST.
- **Rationale**: Spec FR-015 / clarify Q5. Breaks the circular “beyond what 05 implements”.
- **Alternatives considered**: All aggs MUST NOT (weaker ES); full ES agg DSL (second engine).

## R8. Product version vs internodes vs disk format

- **Decision**: `ProductVersion` is a `u16` **major**. First binary ships **1**. A cluster MAY mix majors N and N+1 only (complete product). N+2 handshake on `internode`/`replication` → refuse (`012` already). Disk **format major** in `013`: product N writes format N; product N+1 reads N and N+1, writes N+1 only when the cluster min product is N+1 (or the operator completed upgrade). An N node MUST NOT write format N+1 or create a type whose catalog `introduced_in` is N+1 (`003` catalog-diff). Internodes **frame** version uses the same major as product version (one N).
- **Rationale**: Spec FR-013 / clarify Q4; `012`/`013` already refuse. First binary tests are same-version only.
- **Alternatives considered**: Independent internodes numbering (operators would see two N’s); mixed-version in slices 1–5 (no format evolution yet).

## R9. COPY formats

- **Decision**: Complete-product PostgreSQL COPY MUST: `text`, `csv`, `binary` (`005` `CopyFormat`). First binary: COPY not-supported. `COPY … PROGRAM` and `FREEZE` are MUST NOT.
- **Rationale**: Align with `005` data-model already naming 015. `002` currently lists text/csv only — binary is added at slice 8.
- **Alternatives considered**: Text only (JDBC/pg_dump binary); first-binary COPY (contradicts `016`).

## R10. Not-supported is `002`’s error renderer

- **Decision**: Do not invent new wire errors. `compat::Classify::MustNot { verb }` → existing `ErrorRenderer` (`002` data-model §8): PG `0A000`, Redis `-ERR unknown command`, CQL `0x000A`, HTTP `400`/`501` with ES/CH/S3/WebDAV bodies. Never empty `+OK`, never 200 with empty hits, never PG `T`/`D`/`C` for a refused verb.
- **Rationale**: Spec FR-003; `002` Q on MUST NOT.
- **Alternatives considered**: A parallel error crate (drift).

## R11. Connection admission vs `001` buffers

- **Decision**: `max_connections_per_entrypoint` counts TCP sessions accepted on that listener (including AUTH in progress). `max_connections_per_principal` counts authenticated sessions for that principal id (`014`) across all handlers. Exceed → protocol connection error (PG `53300 too_many_connections`, Redis `-ERR max connections`, HTTP 429/503 with named JSON/XML). Do **not** wait. `001` buffer-full remains a separate named reject (`005` admission already maps it).
- **Rationale**: Spec FR-011; reject not hang.
- **Alternatives considered**: Kill oldest session (surprising); one global connection cap only (hides noisy principal).

## R12. Metrics

- **Decision**: Increment existing `08` families; add values, do not rename labels. `spacestorage_limit_rejected_total{limit,protocol,namespace}` and reuse `query_admission_rejected_total` from `005`. Soft **warning** gauges `spacestorage_limit_usage_ratio{limit}` at 80% MAY exist; enforcement is still hard at 100%.
- **Rationale**: Spec FR-011 soft metrics MAY; constitution X / observability contract.
- **Alternatives considered**: New required label names (would need `08` amendment).

## R13. Catalog-diff of types/formats

- **Decision**: `003` already owns type catalog versioning. This feature requires `TypeDescriptor.introduced_in: ProductVersion` (default 1) and `catalog_diff(from, to)` used by admin (`spacestorage catalog diff --from N --to N+1`). N node create of a type with `introduced_in > N` → refuse. Implementation of the diff listing is `003`; the refuse rule is tested here.
- **Rationale**: Spec FR-013; avoid a second catalog.
- **Alternatives considered**: Compat crate duplicates type names (stale).
