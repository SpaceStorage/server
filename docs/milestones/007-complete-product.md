# Milestone: complete-product (slices 1–11)

**Profile**: `complete-product`  
**Tag**: `slices-1-11`

## Implemented

Slices `1..=11` shipped. Ledger: [007-complete-product.yaml](007-complete-product.yaml).

Slice **11** DoD (`009-admin-ui-ingest`):

- New crates `spacestorage-admin-ui` (Cerebro-like map + Kibana-like console) and `spacestorage-ingest` (Kafka consumer + syslog)
- Admin HTTP: `/ui/*`, `/v1/cluster/map`, `/v1/console/*`, `/v1/ingest/kafka`
- CLI: `spacestorage ui`, `spacestorage ingest kafka|syslog`
- Config: `ingest kafka`, `entrypoint.ingest`, buffers `ingest.syslog.recv` / `ingest.kafka.decode`
- First-binary refuses UI/ingest with `UiIngestSlice11Required`
- Kafka at-least-once (OffsetCommit after durable ack); syslog RFC 5424 preferred / 3164 accepted; 1:1 port→container
- Release profile / Cargo feature `complete-product` is the merge gate for the full matrix (not a synonym for handlers-only)
- Conformance behind `--features complete-product`

## Deferred

None for slices 6–11 (`still_owed` cleared on the tip ledger). Honest residuals that do **not** keep slice 11 `still_owed`:

- Live Kafka broker I/O is covered by a pure-Rust `FakeKafkaBroker` + `kafka-protocol` dep for ALO/offset-commit semantics; production fetch against external brokers can deepen without reopening the slice ledger
- UI principal resolution is interim (bearer → cluster admin until full `014` session wiring on `/ui/*`)
- Earlier-milestone residuals (WAL kill+re-read, PG Document Store blob) remain on `001-first-binary` and do not block `complete-product`

## Changelog

- Added admin-ui and ingest library crates; mounted on admin-http
- Wired slice-11 feature through release-profile, node, spacestorage CLI, and conformance
- Closed the full-product ladder (intent `16` slices 1–11)
