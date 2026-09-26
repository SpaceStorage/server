# Milestone: handlers-complete (slice 6 in progress)

**Profile**: `handlers-complete` (Cargo / dialect) — **not** `complete-product`  
**Tag**: prefer `slices-1-6-wip` — do **not** brand as seven-protocol complete product

## Implemented

Slices `1..=5` shipped (see [001-first-binary](001-first-binary.md)).

Slice **6** (remaining `015` HandlersComplete handlers) is **in progress**:

- Workspace crates: `handler-cassandra`, `handler-elasticsearch`, `handler-clickhouse` (native + HTTP), `handler-s3`, `handler-webdav`
- Feature `handlers-complete` on `node` / `spacestoraged` / `conformance` / `release-profile`
- Classify-before-IR MUST smokes + MUST-NOT refuses; `compat_handlers_complete` suite
- FirstBinary continues to refuse these handler names (`entrypoint_unknown_handler`)

## Deferred

Still owed (intent files stay; `still_owed: true`):

- Slice 6 residuals — full native/SigV4/Digest/CityHash wire dialects + stock-client matrix (see `002` Phase 11 Convergence)
- Slice 7 — Raft, quotas, full authz
- Slice 8 — query beyond CRUD (PG BEGIN/COPY/cursors; ES aggregations)
- Slice 9 — full `008` catalog
- Slice 10 — migration and PITR
- Slice 11 — UIs and ingest

No YAML ledger record claims `implemented: [1..6]` yet — wire DoD for HC MUST dialects is not complete enough to close the slice-6 gate. When closed, add `002-handlers-complete.yaml` with `profile: handlers-complete`, `implemented: [1,2,3,4,5,6]`, deferred `{7..11}`.

## Changelog

- Added HC handler crates with catalog-backed dispatch (classify-gated)
- Node registers HC handlers only under `--features handlers-complete`
- Conformance: `cargo test -p spacestorage-conformance --features handlers-complete`
- `compat::metrics::record_must_not` for T025 counters
