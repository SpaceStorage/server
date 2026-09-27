# Milestone: observability-catalog (slice 9)

**Profile**: `observability-catalog`  
**Tag**: `slices-1-9`

## Implemented

Slices `1..=9` shipped. Ledger: [005-observability-catalog.yaml](005-observability-catalog.yaml).

Slice **9** DoD (`008-observability`):

- Process-wide `Registry` + omit-label Prometheus 0.0.4 encoder in `crates/observability`
- Full catalog families registered (query/API/comm/system/replication/jobs/LSM/HNSW/durability/billing + freshness/export)
- Global `GET /metrics` exposes Four Golden Signals + billing series when owners record; system/buffer/`node_ready`/`node_state` aliases always synced from node
- Tenant `GET /metrics/namespaces/{name}` (404 `metrics_disabled` until enabled); filter isolates namespace
- Aggregation freshness gauges (`spacestorage_shared_aggregation_up` / `last_success_timestamp_seconds`); freeze on primary-down
- Config `metrics { }` / `log { kafka|syslog }` with `ObservabilitySlice9Required` / `SinkConfigInvalid` gates
- Outbound log channels (default/slow_query/audit), pure-Rust Kafka Produce (wire frames + TCP), RFC 5424 syslog, OTLP/HTTP protobuf push
- Conformance: `metrics_first_binary`, `metrics_catalog`, `metrics_tenant`, `logs_sinks`; config fixture tests
- Release profile `observability-catalog` (k=9)

## Deferred

Still owed (`still_owed: true`):

- Slice 10 — migration and PITR
- Slice 11 — UIs and ingest

Do **not** brand this milestone as `complete-product` (slices 10–11 remain).

## Changelog

- Expanded thin `Metrics` (uptime/`queries_total`) into registry/encoder/catalog/sinks
- Wired `admin-http` scrape to live node/buffer/worker system snapshot
- Added `observability-catalog` release-profile + milestone ledger with deferred 10–11
