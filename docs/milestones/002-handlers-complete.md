# Milestone: handlers-complete (slice 6 closed)

**Profile**: `handlers-complete` (Cargo / dialect) — **not** `complete-product`  
**Tag**: `slices-1-6`

## Implemented

Slices `1..=6` shipped. Ledger: [002-handlers-complete.yaml](002-handlers-complete.yaml).

Slice **6** wire DoD (002 T043–T048):

- Elasticsearch HTTP/1.1 + Basic (`GET /`, index, doc, search) — `handler-elasticsearch`
- S3 SigV4 + path-style XML ops (bucket/object/multipart) — `handler-s3`
- WebDAV Basic/Digest + PROPFIND/MKCOL/PUT/GET/MOVE/DELETE — `handler-webdav`
- Cassandra CQL v4/v5 frames + SASL PLAIN — `handler-cassandra`
- ClickHouse native Hello/Query + CityHash 1.0.2 checksum helper; HTTP SQL — `handler-clickhouse`
- Signature/mismatch via `protocol-core` across HC + first-binary handlers (FR-004/FR-005)
- Gate: `cargo test -p spacestorage-conformance --features handlers-complete`
- First-binary remains green: `--features first-binary`

## Deferred

Still owed (`still_owed: true`):

- Slice 7 — Raft, quotas, full authz
- Slice 8 — query beyond CRUD (PG BEGIN/COPY/cursors; ES aggregations)
- Slice 9 — full `008` catalog
- Slice 10 — migration and PITR
- Slice 11 — UIs and ingest

Do **not** brand this milestone as `complete-product`.

## Changelog

- Added `crates/protocol-core` (signature peek, mismatch refusal, HTTP helpers)
- Upgraded HC handler `serve()` paths from line stubs to wire dialects (T043–T047)
- Applied signature detection on redis/postgresql as well as HC handlers (T048)
- Conformance mismatch matrix uses `protocol-core::detect_signature`
- Milestone YAML claims `implemented: [1,2,3,4,5,6]` with deferred 7–11
