# Contract: Implementation slices

**Feature**: `016-mvp-and-nongoals` | Crate: `crates/release-profile` | Spec: FR-001, FR-003

Numeric **specify** order remains `01`–`16`. This table is **implementation** order. Skipping a later slice in a milestone MUST add a `DeferredSlice` ([milestone-record.md](milestone-record.md)); it MUST NOT delete or comment out intent files.

| Slice | Name | Implements (intent / spec) | First binary |
|------:|------|----------------------------|:------------:|
| 1 | Runtime | `001` process, entrypoints, admin CLI/HTTP, buffers, drain | yes |
| 2 | Types + durability | `003` creatable containers (required L3 subset), `013` WAL/restore, `014` master-key file | yes |
| 3 | PostgreSQL subset | `002` `postgresql` handler at [first-binary dialect](dialect-first-binary.md) | yes |
| 4 | Redis subset | `002` `redis` handler; `015` Redis MUST list on `K/V Store` | yes |
| 5 | Membership + internode + quorum | `011` identity/join, `012` fabric/`quorum_domain`, `004` replication/quorum only (not full planetary examples) | yes |
| 6 | Remaining handlers | `002` cassandra, elasticsearch, clickhouse, clickhouse-http, s3, webdav at the **`015` MUST subset** — not a private smaller list | deferred after 1–5 |
| 7 | Control plane / tenancy / authz | `006` Raft, `007` quotas, `014` full permission vocabulary | deferred |
| 8 | Query beyond CRUD | `005` joins, aggregation, MapReduce, subscribe, distributed transactions; PG `BEGIN` as `015` | deferred |
| 9 | Observability catalog | `008` series live on `/metrics` (series MAY appear earlier as paths exist) | deferred |
| 10 | Migration / backup | `010` transforms, `013` snapshot/PITR | deferred |
| 11 | UIs and ingest | `009` Cerebro-like / Kibana-like UIs; Kafka/syslog ingest | deferred |

## Prefix rule

Implemented slices MUST be `1..=k` with no holes (`slice_gap`). A 1–5 milestone has `k = 5` and deferred `{6,7,8,9,10,11}`.

## Slice 6 special rule

When slice 6 is marked implemented, remaining handlers MUST pass the complete-product MUST column in `015`, not an unpublished subset. First-binary dialects MAY remain available behind `ReleaseProfile::FirstBinary` for compatibility tests, but the complete-product profile is the slice-6 gate.
