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

**Still thin / not DoD-complete**:

- `first_binary.rs` G1/G2/G6/G7/G10 and `document_store.rs` G11 still call
  `in_process_cluster_ready()` (always `true`) — real cluster proof lives in
  `_smoke_boot` only.
- `handlers_absent.rs` asserts the profile inventory; it does not yet boot
  `unknown-handler-cassandra.conf` through validate/startup.
- Document Store admin-create + canonical-blob CRUD gate (G11) is not a real
  end-to-end test yet.
- Quickstart operator timing (T051) and clippy clean (T052) not recorded here.
