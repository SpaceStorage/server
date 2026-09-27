# Milestone: first-binary (slices 1–5)

**Profile**: `first-binary`  
**Tag**: `slices-1-5`

This record tracks the first shippable binary. It is **not** a “v1” of the
seven-protocol matrix.

## Implemented

- Slice 1 — Runtime (`001`)
- Slice 2 — Types + durability (`003`, `013`, `014` master-key)
- Slice 3 — PostgreSQL subset (`002`)
- Slice 4 — Redis subset (`002`, `015` KV MUST)
- Slice 5 — Membership + internode + quorum (`011`, `012`, `004`)

## Deferred

These slices remain owed (`still_owed: true`); intent files `01`–`15` are not deleted.

- Slice 6 — remaining `015` handlers (Cassandra, ES, ClickHouse, S3, WebDAV)
- Slice 7 — Raft control plane, quotas, full authz vocabulary
- Slice 8 — query beyond CRUD
- Slice 9 — full `008` observability catalog
- Slice 10 — migration / transforms and PITR
- Slice 11 — admin UIs and Kafka/syslog ingest

## Changelog

### 2026-09-26 — Post-ladder: WAL re-read + PG Document Store blob

**Proven green**:

- WAL kill→reboot→re-read of Redis client content (`wal_kill_reboot`) via durable catalog hooks + `definitions.jsonl` hydrate
- Document Store blob via Redis (G11) **and** PostgreSQL INSERT/SELECT (`g11_document_store_blob_via_postgresql`)

Earlier G6/G7 quorum-math residual and “PG blob not gated” notes below are closed by this entry.

### 2026-09-26 — Phase 0 DoD: real G gates + Document Store blob

**Proven green** (`CARGO_TARGET_DIR=target`):

- `cargo test -p spacestorage-conformance --features first-binary` — G1–G11
  no longer stub on `in_process_cluster_ready()`:
  - G1/G2/G6/G7/G10 via `boot_one_node` / `boot_three_node` (+ omitted-transport validate, drain refuses new tenant accepts)
  - G8 validates `unknown-handler-cassandra.conf` (and ES/CH/S3/WebDAV rewrites) → `entrypoint_unknown_handler`
  - G11 admin-create Document Store on live node catalog + Redis GET/SET blob; `JSON.GET` errors
- `crates/conformance/tests/quickstart.rs` records SC-001–SC-003/SC-006 quickstart steps as executable tests
- Redis data verbs use `ensure_data` (auto-create K/V; allow admin-created Document Store as blob)
- `cargo test -p spacestorage-release-profile --test ledger` green; deferred 6–11 still `still_owed: true`
- `rustfmt`/`clippy -D warnings` clean on `release-profile`, conformance (`--no-deps`), handler-redis

**Still thin / honest residuals** (superseded by post-ladder entry above when green):

- G6/G7 proves durable-ack quorum math (TWO ≠ `min(2, live)`); kill→reboot WAL re-read tracked as post-ladder
- Document Store blob proven on Redis; PostgreSQL blob mapping tracked as post-ladder
- Slice 6+ handlers remain deferred on this milestone ledger (closed on tip `007`)

### 2026-09-26 — conformance + Redis SET + membership roster

**Proven green** (`CARGO_TARGET_DIR=target`):

- `cargo test -p spacestorage-conformance --features first-binary` — including
  `_smoke_boot` one-node (write ONE) and three-node (membership converge +
  write TWO ≠ `min(2, live)`).
- Redis MUST list (`dialect_redis`): SET/GET/DEL/EXISTS/SCAN after K/V path
  switched from `ensure_blob` (no auto-create) to `ensure_kv`.
- PostgreSQL first-binary dialect unit tests (`dialect_pg`): CRUD +
  BEGIN/COMMIT/ROLLBACK/COPY → `0A000`.
- `multi_active=on` refused at catalog create (`multi_active`).
- Release-profile ledger + nongoals tests green; this YAML still lists
  deferred slices 6–11 with `still_owed: true`.

**Membership note**: without Raft (`006`), joiners refresh the seed roster
via `MembershipService::refresh_roster_from_seeds` so later admits reach
earlier members (FR-009). Not a full view-push / cluster-log apply.
