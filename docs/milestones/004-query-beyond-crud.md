# Milestone: query-beyond-crud (slice 8)

**Profile**: `query-distributed`  
**Tag**: `slices-1-8`

## Implemented

Slices `1..=8` shipped. Ledger: [004-query-beyond-crud.yaml](004-query-beyond-crud.yaml).

Slice **8** DoD (`005-query-execution`):

- Shared `PlannerEngine` in `crates/query` (spec paths historically said `crates/exec`) — stages, IR, admission, ranking, spill, cancel/timeout
- Production records label `engine = "planner"`; handlers call shared planner (no second engine per protocol)
- Cargo feature `query-distributed`: join (inner/left), aggregation (COUNT/SUM/MIN/MAX/AVG + GROUP BY), MapReduce + shuffle transport, subscribe/job types
- Complete-product dialect: PG `BEGIN`/`COMMIT`/`ROLLBACK`, `COPY`, forward-only cursors (`DECLARE`/`FETCH`/`CLOSE`); `SERIALIZABLE` / `LISTEN`/`NOTIFY` / `WITH HOLD`/`SCROLL` refused
- First-binary / handlers-complete continue to refuse BEGIN/COPY/cursors before IR
- Config `query { }` validation codes (`query_max_concurrent_zero`, `query_max_memory_zero`, `query_concurrency_zero`, `query_spill_unknown`)
- Internode additive shuffle/job message ids (30–34)
- Release profile `query-distributed` (k=8)

## Deferred

Still owed (`still_owed: true`):

- Slice 9 — full `008` catalog
- Slice 10 — migration and PITR
- Slice 11 — UIs and ingest

Do **not** brand this milestone as `complete-product` (slices 9–11 remain).

## Changelog

- Grew `crates/query` from stub into planner/executor + `query-distributed` engines
- PostgreSQL handler routes SQL through `PlannerEngine` / `lower_sql`
- Config query-block validation; internode shuffle RPC kinds
- Milestone YAML claims `implemented: [1..8]` with deferred 9–11
