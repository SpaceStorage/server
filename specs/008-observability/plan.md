# Implementation Plan: Observability, Billing Metrics, and Logging

**Branch**: `008-observability` | **Date**: 2026-09-16 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/008-observability/spec.md` (Clarifications, Session 2026-09-16 — default lifecycle/failure logs plus optional slow-query and audit channels; omit unknown labels; tenant scrape and OTel both implemented; Kafka and syslog both implemented; last aggregates + dedicated freshness when the aggregator primary is down)

## Summary

Own the **in-memory metric registry**, **Prometheus/OTLP exposition**, and **outbound logs** for SpaceStorage. Other features **record** figures; this crate **names, stores, filters, and exports** them. Global `GET /metrics` on `admin-http` (404 in `001`) becomes the Prometheus scrape. Shared-datatype series are merged on the **namespace primary** via existing `006` `MetricsPush`, copied to secondaries (`MetricsSnapshot`), and marked stale with a dedicated freshness pair — not a `stale` label and not by dropping series. Tenants get a namespace-filtered scrape and/or OTel push. Logs default to lifecycle and failures; slow-query and audit export are off until enabled. Sinks are Kafka and syslog only.

**First binary** (`016` slices 1–5): Prometheus text on global `/metrics` for **implemented-path** series (`001` node/buffer/worker names, plus any other reserved names already incremented). **Slice 9**: full intent catalog, tenant surfaces, OTel, Kafka/syslog, log channels, aggregation freshness conformance.

## Technical Context

**Language/Version**: Rust 1.87 (edition 2024, MSRV 1.85), same Cargo workspace as `001`–`016`.

**Primary Dependencies**: existing workspace (`tokio`, `axum`/`hyper` for scrape routes, `serde`/`serde_json`, `tracing`/`tracing-subscriber`, `bytes`, `parking_lot`, `rustls`). Slice 9 optional: `opentelemetry` + `opentelemetry-otlp` (`http-proto` only), pure-Rust Kafka (`rskafka` or in-crate Produce). **Forbidden**: `rdkafka`/`librdkafka`, OpenSSL, `prometheus` `CounterVec` as the omit-label store (R2). Internodes additive `MetricsSnapshot` (`006` already has `MetricsPush`).

**Storage**: All stats **in memory** (constitution X). No Raft log for samples. `NamespaceTelemetry` (sink URLs, scrape/otel enable bits) is a cluster-log body interpreted here, applied on every node (`006`/`007` cluster store). Lost aggregates rebuild from `MetricsPush` after election (`006`).

**Testing**: `cargo test`. Unit: omit-label encoder, tenant filter, channel gating, freshness freeze. `crates/conformance`: first-binary `/metrics` 200 + `001` buffer names; slice-9 tenant isolation, Kafka/syslog delivery, primary-down freshness. Contract tests on `contracts/fixtures/` and catalog golden snippets.

**Target Platform**: Linux x86_64/aarch64 servers (primary), macOS for development. Multi-node aggregation tests: in-process loopback, same harness as `006`.

**Project Type**: Cargo workspace extension — **one new library crate** `crates/observability` (`spacestorage-observability`). No new binary. `admin-http` / optional `metrics` handler call this crate. `release-profile`: first binary compiles registry+`/metrics`; slice 9 compiles OTLP/Kafka/syslog/tenant.

**Performance Goals**: `/metrics` encode of a loaded node < 50 ms p95 at catalog cardinality of one namespace × tens of datatypes (SC-005 operator path). Record path (counter inc) < 200 ns p99 after handle intern. OTLP/Kafka I/O on Tokio; never on the query worker critical path (bounded mpsc, drop+`log_export` error metric/log on overflow). Leaderless KV p95 unchanged vs `004` when observability is idle.

**Constraints**: Omit unknown labels; never invent; never empty token. Do not rename `db_*` intent series or `001`/`006`/`007` reserved `spacestorage_*` names. Tenant scrape: 0 foreign-namespace series. Kafka and syslog only. Audit content is `14`. Key material never in logs. First binary MUST NOT require tenant/OTel/Kafka. Custom encoder required for omit-label (Complexity Tracking).

**Scale/Scope**: One registry per process. Cardinality dominated by `(namespace, schema, datatype, kind)` plus optional `user`/`application` when known. Tens to hundreds of namespaces; high-cardinality user labels only when authenticated identity exists. Roughly: registry+encoder (~2 k), HTTP/OTLP (~1.5 k), logs/sinks (~2 k), aggregation freshness (~1 k), catalog tests (~2 k); ≈ 8–12 k lines including tests.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Gate | Status |
|---|-----------|------|--------|
| I | Rust-Only Stack | New crate Rust; OTLP HTTP; pure-Rust Kafka; rustls; no `rdkafka` | PASS |
| II | Fully Asynchronous Tokio Runtime | Scrape is async axum; OTLP/Kafka/syslog async; record path non-blocking; overflow drops rather than block workers | PASS |
| III | Single-Process Multithreaded Monolith | Library inside `spacestoraged`; no metrics sidecar | PASS |
| IV | Type-Driven Multiparadigm | No new datatypes. Catalog labels `datatype` / `storage_type` select `03` names | PASS |
| V | Protocol Compatibility on Distinct Ports | No new client protocol. Optional `metrics` handler is one handler per entrypoint | PASS |
| VI | Every Node Is a Request Coordinator | Every node serves local `/metrics`; aggregation is extra on namespace voters | PASS |
| VII | Label-Based Planetary Placement | Unchanged. No planet as a metric domain | PASS |
| VIII | Cassandra-Style Quorum | Unchanged. Metrics are not quorum | PASS |
| IX | Multi-Tenant Namespaces | Per-namespace scrape/push/logs vs global admin streams | PASS |
| X | Observability as a Product Surface | **This feature is the principle.** Prometheus names, OTel+Prometheus, Four Golden Signals, in-memory, billing labels, buffer usage | PASS |
| XI | Documented, Expandable Configuration | Starters for `/metrics`, tenant scrape, OTel, Kafka, syslog; `metrics` block reserved in `001` | PASS |
| XII | Raft Controller Elections and Local Restore | Consumes `006` primary + `MetricsPush`; freshness series; does not store samples in Raft | PASS |
| XIII | Security Defaults for Data and Roles | `METRICS_READ` / `AUDIT_READ` from `14`; keys never in logs | PASS |
| Arch. Contracts | Five-level stack; adapter ≠ type system | Observability is not a datatype. Handlers still `002` → exec | PASS |
| Observability Contract | Intent labels/series | Catalog maps prose → names; `db_*` and reserved `spacestorage_*` not renamed; freshness pair is **added** (FR-018) | PASS |

**Gate result (pre-research)**: PASS. Slice 9 for tenant/OTel/sinks matches `016`. First-binary `/metrics` is already required by `016` and reserved in `001`.

## Project Structure

### Documentation (this feature)

```text
specs/008-observability/
├── plan.md                         # This file
├── research.md                     # Phase 0: R1–R16
├── data-model.md                   # Phase 1: registry, sample, telemetry, log event, freshness
├── quickstart.md                   # Phase 1: scrape, tenant, freshness, sinks
├── contracts/
│   ├── catalog.md                  # series + labels + buckets (FR-001–015, FR-018)
│   ├── exposition.md               # /metrics encode, omit-label, tenant filter (FR-016, FR-023)
│   ├── otel.md                     # OTLP/HTTP push (FR-016)
│   ├── logs.md                     # channels, Kafka, syslog (FR-017, FR-020–022)
│   ├── aggregation.md              # MetricsPush/Snapshot, freshness (FR-003, FR-024)
│   ├── admin-http-metrics.md       # route deltas vs 001
│   ├── config-directives.md        # metrics/log/otel/namespace telemetry
│   ├── metrics.md                  # names this crate adds; owners of increments
│   └── fixtures/
│       ├── README.md
│       ├── metrics-block.conf
│       └── invalid/
├── checklists/requirements.md
└── tasks.md                        # Phase 2 (/speckit-tasks — NOT created here)
```

### Source Code (repository root)

```text
crates/
├── observability/                           # spacestorage-observability — CORE
│   └── src/
│       ├── lib.rs                           # Registry, Recorder
│       ├── sample.rs                        # Name + BTreeMap labels + Counter/Gauge/Histogram
│       ├── encode.rs                        # Prometheus text 0.0.4 / OpenMetrics
│       ├── filter.rs                        # tenant namespace filter
│       ├── http.rs                          # axum handlers: /metrics, /metrics/namespaces/{name}
│       ├── otel.rs                          # OTLP/HTTP push (slice 9)
│       ├── logs.rs                          # channels; SinkLayer
│       ├── kafka.rs                         # pure-Rust producer (slice 9)
│       ├── syslog.rs                        # RFC 5424 UDP/TCP (slice 9)
│       ├── aggregation.rs                   # merge + freshness gauges
│       └── catalog.rs                       # registered families / help text
│
├── node/                                    # stats.rs registers with Registry; admin-http routes
│                                            # optional handler metrics
├── internode/                               # additive MetricsSnapshot (006 already MetricsPush)
├── controlplane/                            # cluster-log body NamespaceTelemetry
├── tenancy/                                 # private_* flags: slice 9 wiring vs ObservabilitySlice9Required
├── exec/ | types/ | wal/ | placement/       # increment catalog families they own
├── config/                                  # metrics { … }; log kafka/syslog; namespace telemetry
├── admin-proto/ | spacestorage/             # optional: metrics enable CLI (slice 9)
├── release-profile/                         # first binary: /metrics; slice 9: full export
└── conformance/                             # scrape, tenant, freshness, sinks
```

**Structure Decision**: New `observability` crate so `node` stays process/lifecycle (`001`) and `controlplane` stays Raft (`006`). First binary still has `admin-http`; turning `/metrics` from 404 to 200 is the slice-1-compatible cut.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| Custom Prometheus encoder instead of `prometheus` crate vecs | Clarify Q2: omit unknown label keys | `CounterVec` forces a fixed label set or empty tokens, both illegal |
| `MetricsSnapshot` in addition to `006` `MetricsPush` | Clarify Q5: last aggregates remain when primary is down | Keeping the map only on the leader loses values on crash; dropping series was option C |
| Optional Kafka/OTLP deps behind slice 9 | Constitution I + `016` first binary | Always-link `rdkafka` is C; always-link OTLP bloats slices 1–5 |

## Constitution Check (post-design)

Re-evaluated after Phase 1: registry is in-process memory; scrape is axum on existing or dedicated handler; OTLP is HTTP/protobuf Rust; Kafka is pure Rust; tenant filter is namespace label; freshness is an additive series pair (not a rename); audit fields stay `14`; keys never in sinks; first binary only requires global `/metrics`. **Gate result: PASS.**
