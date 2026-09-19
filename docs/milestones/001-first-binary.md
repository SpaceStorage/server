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

Initial ledger for the slices-1-5 / first-binary release profile.
