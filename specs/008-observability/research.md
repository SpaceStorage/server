# Research: Observability, Billing Metrics, and Logging

**Feature**: `008-observability` | **Date**: 2026-09-16

Each item resolves a Technical Context unknown or a technology choice. Format: Decision / Rationale / Alternatives considered. Inputs: [spec.md](spec.md) (clarify 2026-09-16), constitution 1.3.0, intent `08`, sibling plans `001` (stats names, `/metrics` 404), `006` (`MetricsPush`, namespace-primary merge), `007` (`private_metrics` / `private_logs`), `009` (RFC 5424 ingest), `014` (`METRICS_READ`, `AUDIT_READ`, audit events), `016` (first binary `/metrics`; slice 9 full catalog).

## R1. One crate owns the in-memory store and exposition

- **Decision**: Add `crates/observability` (`spacestorage-observability`). Other crates **increment** via a `Recorder` trait (atomics + histogram observations). This crate **encodes** Prometheus text, filters tenant series, pushes OTLP, and ships logs. It does not own job behavior, WAL, or Raft.
- **Rationale**: Constitution X (in-memory stats per node). `001` already keeps figures in `stats.rs` with reserved names; moving exposition here avoids every crate depending on OTLP/Kafka. Tests of omit-label encoding must not boot Kafka.
- **Alternatives considered**: Grow `node` (`001`) with Prometheus (rejected: catalog and sinks dwarf runtime); one crate per family (rejected: one scrape, one registry).

## R2. Custom series registry, not `prometheus` `CounterVec`

- **Decision**: Store samples as `(name, BTreeMap<label,value>) → sample` with atomic counters/gauges and lock-free histogram buckets. Encode Prometheus text 0.0.4 / OpenMetrics 1.0 ourselves. **Omit** a label key when its value is unknown (clarify Q2). Do not emit `""` or `unknown`.
- **Rationale**: `prometheus::IntCounterVec` requires a fixed label set on every series of a metric. Clarify forbids empty tokens, so a vec with always-present `user=""` is illegal. Prometheus text **allows** the same name with different label keys; that is the product rule.
- **Alternatives considered**: Always-on empty labels (rejected: Q2 option B/C); two metric families per optional label (rejected: combinatorial); `metrics` + exporter (same fixed-label issue).

## R3. First binary vs slice 9

- **Decision**: First binary (`016` slices 1–5): `GET /metrics` on `admin-http` returns **200** with series that implemented paths already increment (`001` buffer/node/worker, plus any other reserved names that exist). Missing catalog families are **absent** (not zero) until their owner crate records them. Tenant scrape, OTel push, Kafka/syslog outbound, slow-query/audit channels, and aggregation freshness **conformance** wait for **slice 9**. Config that enables those on a first-binary profile → `ObservabilitySlice9Required` (same pattern as `007` `Slice7Required`).
- **Rationale**: `016` first binary already requires global `/metrics`; slice 9 is “catalog live”. Series MAY appear earlier (`016` slices contract).
- **Alternatives considered**: 404 until slice 9 (rejected: first-binary MUST); full catalog zeros on day one (rejected: noise and false SLO).

## R4. Global scrape stays on `admin-http`; optional `metrics` handler

- **Decision**: Implement reserved `GET /metrics` on `admin-http` (`001` currently 404). Auth: same bearer as admin until `14`; then `METRICS_READ` (cluster) for global, `METRICS_READ` on that namespace for tenant. Optional entrypoint `handler metrics;` (config word already reserved in `001`) speaks **only** metrics routes (no `/v1/*`). Same body as `admin-http` metrics paths. First binary does not require the `metrics` handler.
- **Rationale**: `001` already reserved the path on the existing HTTP stack (`axum`). A scrape-only port is the usual Prometheus bind (`:9100` style) without exposing reload/stop.
- **Alternatives considered**: Metrics-only from day one (rejected: extra mandatory handler vs `001` starter); OpenMetrics-only protobuf (rejected: text scrape is the Prometheus default).

## R5. Tenant filter is `namespace=<name>` only

- **Decision**: Tenant scrape `GET /metrics/namespaces/{name}` and tenant OTel push include **only** series that have label `namespace` equal to that tenant’s **current name**. Node-global series (uptime, worker threads, buffer without namespace) stay on **global** scrape only. Foreign-namespace series are omitted. After `007` rename, the label follows the new name; `namespace_id` MAY be added (not a required intent label).
- **Rationale**: Clarify Q3 isolation; `007` rename rules. Spec SC-003: 0 foreign series.
- **Alternatives considered**: Include node-global series on tenant scrape (rejected: leaks cluster saturation to a tenant); filter by `namespace_id` only (rejected: operators scrape by name).

## R6. OTLP/HTTP is the required OTel transport

- **Decision**: Complete product implements **OTLP/HTTP** (protobuf) via `opentelemetry` + `opentelemetry-otlp` (`http-proto` feature), all Rust. Global push from node config `metrics.otel`. Per-namespace push from `NamespaceTelemetry`. Interval default 15 s. OTLP/gRPC is not required. Map the same in-memory registry to OTLP `ResourceMetrics` (resource: `service.name=spacestorage`, `service.instance.id=node_id`).
- **Rationale**: Constitution names OpenTelemetry as a primary system. HTTP avoids `tonic`/C-gRPC. Same series set as scrape (SC-003).
- **Alternatives considered**: gRPC-only (rejected: extra runtime); Prometheus remote-write as OTel substitute (rejected: constitution).

## R7. Kafka producer is pure Rust

- **Decision**: Produce to Kafka with a **pure-Rust** client (`rskafka` or `kafka-protocol` Produce in this crate). **Not** `rdkafka` / `librdkafka` (constitution I). TLS via `rustls`. At-least-once; no transactions. Topic is config. Key: namespace name (or `_cluster` for global). Value: JSON log object ([logs.md](contracts/logs.md)).
- **Rationale**: Constitution I. `009` uses Kafka for ingest; outbound is a producer only.
- **Alternatives considered**: `rdkafka` (rejected: C); sidecar vector/fluent (rejected: monolith must emit).

## R8. Syslog outbound is RFC 5424

- **Decision**: UDP and TCP syslog, RFC 5424 framing (octet-counted on TCP). RFC 3164 is **not** required for outbound (ingest `09` still accepts 3164). Facility `local0`, severity from channel (errors=`err`, lifecycle=`info`, slow-query=`notice`, audit=`notice`). APP-NAME `spacestorage`. Structured data: `ss@32473` with `ns`, `channel`, `node`.
- **Rationale**: Clarify Q4; match `09` preferred ingest so a loop tenant→us is parseable.
- **Alternatives considered**: 3164 outbound (rejected: unstructured); journald-only (rejected: not a tenant sink).

## R9. Aggregation freshness supersedes `006` “absent or stale”

- **Decision**: Keep `006` `MetricsPush` (members → **namespace** primary, interval = `cluster.raft.heartbeat`). Primary merges in memory. **Additionally**, primary sends `MetricsSnapshot` to namespace **secondaries** each interval so last merged values survive primary crash (load-balancing / failover). Exposition:
  - `spacestorage_shared_aggregation_up{namespace,datatype}` = 1 while this node’s merge is current (leader applying pushes; no replica silent > `2 * heartbeat`); else 0.
  - `spacestorage_shared_aggregation_last_success_timestamp_seconds{namespace,datatype}` = Unix seconds of last successful merge; **frozen** while up=0.
  - Last merged series **remain**; they are **not** dropped because the primary is down; they are **not** labelled `stale`.
  - Every member still exposes **local** series. Cluster primary does not merge (unchanged `006`).
- **Rationale**: Clarify Q5 option A. `006` contract allowed “absent or marked stale”; this feature picks last-values + dedicated freshness. Lost on **all** namespace voters down: local scrapes continue, merged series only on hosts that still have a snapshot.
- **Alternatives considered**: Drop merged series (Q5 C); `stale=true` on every series (Q5 B); scrape-side join (rejected: constitution “stored in memory” on the primary).

## R10. Histogram buckets

- **Decision**: Duration (`*_duration_seconds`): `0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10, 30, 60`. Size (`*_bytes` histograms): `64, 256, 1024, 4096, 16384, 65536, 262144, 1048576, 4194304, 16777216, 67108864`. Documented in [catalog.md](contracts/catalog.md). Plans MAY add buckets (constitution: add series/buckets, do not rename labels).
- **Rationale**: Spec deferred exact types to planning. Prometheus default 10s max is short for compaction/election.
- **Alternatives considered**: Prometheus default buckets (too short); native histograms only (scrapers not universal).

## R11. Log channels on `tracing`, sinks subscribe

- **Decision**: Default lifecycle/failure lines are `tracing` events with target `spacestorage::ops` and field `channel=default`. Slow-query: `channel=slow_query` when enabled and duration ≥ threshold. Audit: `channel=audit` when enabled; **fields** come from `14` (principal, action, target, time); this crate only forwards. A `SinkLayer` fans out to global/namespace Kafka and syslog. Successful under-threshold queries MUST NOT log. Key material NEVER logged (`007` FR-011).
- **Rationale**: Clarify Q1. `001` already uses `tracing`. Audit vocabulary stays `14`.
- **Alternatives considered**: Duplicate audit store in `08` (rejected: `14`); log every request (rejected: Q1 option B).

## R12. Slow-query threshold default

- **Decision**: Default `metrics.slow_query.threshold 1s;` off by default (`enabled off`). Live-reloadable. Applies to query execution duration (same clock as `spacestorage_query_duration_seconds`).
- **Rationale**: Spec left milliseconds to planning. 1 s is a common database default; operators lower it.
- **Alternatives considered**: 100 ms (noisy on planetary RTT); no default (invalid config).

## R13. Authz for scrape and audit export

- **Decision**: Global `/metrics` requires `METRICS_READ` at cluster scope (`14`; first binary: admin token). Tenant path requires `METRICS_READ` for that namespace. Enabling audit **export** does not grant tenants cluster audit: tenant sinks get audit lines **only** for that namespace; cluster-wide audit lines require `AUDIT_READ` to **configure** global audit export (`14`). Scrape still does not return log lines.
- **Rationale**: Clarify Q1/Q3; `014` FR-003/012.
- **Alternatives considered**: Unauthenticated `/metrics` (rejected: multi-tenant); tenant sees cluster audit when they enable their sink (rejected: Q1 isolation).

## R14. Intent names → Prometheus names

- **Decision**: Keep every **already spelled** intent identifier (`db_wal_*`, `db_lsm_*`, `db_hnsw_*`, `db_checkpoint_*`, `db_dirty_*`, `db_unflushed_bytes`, `db_recovery_*`, `node_ready`, `node_state`). Map prose families to `spacestorage_*` names in [catalog.md](contracts/catalog.md). Do **not** rename `001`/`006`/`007` reserved names. `buffer limit hits` = `spacestorage_buffer_limit_hits_total` (already reserved).
- **Rationale**: Observability contract: plans MAY add, MUST NOT rename required labels/series. Mixing `db_*` (intent) and `spacestorage_*` (prior plans) is the product catalog.
- **Alternatives considered**: Prefix everything `spacestorage_` (rejected: would rename intent `db_*`).

## R15. Cargo / release-profile

- **Decision**: First binary links `observability` and serves `/metrics`. Slice 9 profile enables tenant routes, OTel, Kafka, syslog, log channels, `MetricsSnapshot`, freshness series as **conformance-required**. Code may live in the crate behind `cfg`/`ReleaseProfile::Complete` checks so first binary stays small on deps: Kafka/OTLP crates are **optional deps** compiled on slice 9.
- **Rationale**: Pure-Rust Kafka + OTLP should not burden slices 1–5.
- **Alternatives considered**: Always link Kafka (rejected: first binary).

## R16. `001` stats.rs becomes a recorder backend

- **Decision**: `node` `stats.rs` keeps atomics; it registers them on `observability::Registry` at boot (or implements `Recorder`). `/v1/buffers` JSON stays `001`. Scrape and CLI percent MUST match within one sample interval (SC-004). Sample interval default 1 s for derived ratios.
- **Rationale**: SC-004; do not duplicate buffer counters.
- **Alternatives considered**: Reimplement buffers in `08` (rejected: drift).

No `NEEDS CLARIFICATION` remains in Technical Context.
