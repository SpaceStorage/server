# Implementation Plan: Cluster Admin UIs and Log Ingest

**Branch**: `009-admin-ui-ingest` | **Date**: 2026-09-17 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/009-admin-ui-ingest/spec.md` (Clarifications, Session 2026-09-17 — map visibility by role; Kafka as declared consumer; syslog 1:1 listen port; Kafka vs syslog authz; Kafka payload format default raw / optional JSON)

## Summary

Serve a **Cerebro-like cluster map** and a **Kibana-like namespace/cluster console** from the monolith’s existing `admin-http` handler (Rust templates + checked-in vanilla JS, poll every 2 s). Map JSON is a filter over `004`/`006`/`010`/`011`, not a private topology. Console queries and DDL go through `005` with application `spacestorage-ui`.

**Ingest** is a separate crate: Kafka is a **pure-Rust consumer group** (offset commit only after durable `append` ack); syslog is the reserved `001` `syslog` handler, one listen entrypoint per declaration. Default Kafka payload is raw UTF-8; JSON mapping is optional. Outbound Kafka/syslog stays `008`.

**First binary** (`016` slices 1–5): no UI, no ingest (`UiIngestSlice11Required` if configured). **Slice 11**: this plan.

## Technical Context

**Language/Version**: Rust 1.87 (edition 2024, MSRV 1.85), same Cargo workspace as `001`–`016`.

**Primary Dependencies**: existing workspace (`tokio`, `axum`/`hyper`, `serde`/`serde_json`, `rustls`/`tokio-rustls`, `tracing`, `bytes`). UI: `maud` or `askama` (pure Rust). Kafka: `rskafka` and/or `kafka-protocol` (pure Rust). **Forbidden**: `rdkafka`/`librdkafka`, OpenSSL, npm/webpack as a build step.

**Storage**: Kafka ingest declarations in **cluster controller storage** (`006`). Syslog bind in **node config** (entrypoint child). Log records in the target container (`003` `log_stream` `append` → `013` WAL). UI is stateless beyond the session principal.

**Testing**: `cargo test`. Unit: RFC 5424/3164 parse, raw vs JSON Kafka mapping, map tenant filter, buffer drop. `crates/conformance`: three-node lag map; namespace-only map; Kafka at-least-once (kill after ack before commit); syslog 1:1 ports; WRITE-only ingest refused. Contract tests on `contracts/fixtures/`.

**Target Platform**: Linux x86_64/aarch64 servers (primary), macOS for development. Multi-node: in-process loopback, same harness as `006`.

**Project Type**: Cargo workspace extension — **two library crates** `crates/admin-ui` and `crates/ingest`. No new binary. Optional Kafka/syslog deps compiled on slice 11 (`release-profile`).

**Performance Goals**: Map poll 2 s; SC-001 operator finds lag in < 2 min. Ingest encode/parse of one record < 1 ms p95 excluding durable wait. Syslog/Kafka enqueue never blocks a Tokio worker (R7). Leaderless KV p95 unchanged vs `004` when ingest is idle.

**Constraints**: UIs enforce `014` (interim: `001` admin token). Kafka offset never advances past an unacked message. Syslog ports unique (`001`). No hostname routing. `WRITE` alone cannot declare ingest. First binary MUST NOT require UI/ingest. Do not share series or code paths with `008` export.

**Scale/Scope**: Tens of syslog entrypoints per node; tens to hundreds of Kafka declarations per cluster; UI is operator/tenant QPS. Roughly: ingest parse+consumer (~3 k), syslog handler (~1.5 k), admin-ui (~3 k), contracts/tests (~3 k); ≈ 10–12 k lines including tests.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Gate | Status |
|---|-----------|------|--------|
| I | Rust-Only Stack | Crates and Kafka/TLS deps Rust; templates Rust; JS is checked-in assets not Cargo deps; no `rdkafka` | PASS |
| II | Fully Asynchronous Tokio Runtime | Syslog/Kafka/UI on Tokio; overflow drop; durable wait is `013` blocking pool | PASS |
| III | Single-Process Multithreaded Monolith | UI and ingest inside `spacestoraged`; no sidecar | PASS |
| IV | Type-Driven Multiparadigm | Default L3 `Log Stream`; append via type ops; UI not a second type system | PASS |
| V | Protocol Compatibility on Distinct Ports | Syslog is one handler per entrypoint; UI is not a client protocol | PASS |
| VI | Every Node Is a Request Coordinator | UI on every `admin-http`; Kafka group members are all `ready` nodes | PASS |
| VII | Label-Based Planetary Placement | Unchanged; map displays `004` labels | PASS |
| VIII | Cassandra-Style Quorum | Ingest `append` uses container write quorum / durable filter | PASS |
| IX | Multi-Tenant Namespaces | Map/console/ingest filtered by namespace; syslog 1:1 port | PASS |
| X | Observability as a Product Surface | New ingest series; UI application label; no rename of `008` export series | PASS |
| XI | Documented, Expandable Configuration | `ingest` reserved in `001`; starters in fixtures | PASS |
| XII | Raft Controller Elections and Local Restore | Kafka declarations in cluster log; syslog in node config restore | PASS |
| XIII | Security Defaults for Data and Roles | `014` verbs; syslog `CLUSTER_ADMIN`; Kafka `NAMESPACE_ADMIN`+`WRITE` | PASS |
| Arch. Contracts | Five-level stack; adapter ≠ type system | Syslog handler is a protocol adapter to `append`; UI uses `005` | PASS |
| Observability Contract | Intent labels/series | Additive ingest series only; export names untouched | PASS |

**Gate result (pre-research)**: PASS. Slice 11 deferral matches `016`. Static JS assets are not Cargo dependencies (Complexity Tracking).

## Project Structure

### Documentation (this feature)

```text
specs/009-admin-ui-ingest/
├── plan.md                         # This file
├── research.md                     # Phase 0: R1–R15
├── data-model.md                   # Phase 1: map view, ingest declaration, log record
├── quickstart.md                   # Phase 1: map, console, kafka, syslog
├── contracts/
│   ├── ui-http.md                  # /ui/* and /v1/cluster/map, /v1/console/*
│   ├── ingest-declaration.md       # Kafka cluster object + syslog entrypoint child
│   ├── kafka-consumer.md           # group, fetch, offset commit after durable ack
│   ├── syslog.md                   # handler, RFC 5424/3164, 1:1 port
│   ├── log-record.md               # stored fields, raw vs JSON
│   ├── config-directives.md        # ingest kafka; syslog entrypoint ingest {}
│   ├── metrics.md                  # series this crate adds vs 008 export
│   ├── cli.md                      # spacestorage ingest / ui hints
│   └── fixtures/
│       ├── README.md
│       ├── ingest-kafka.conf
│       ├── ingest-syslog.conf
│       └── invalid/
├── checklists/requirements.md
└── tasks.md                        # Phase 2 (/speckit-tasks — NOT created here)
```

### Source Code (repository root)

```text
crates/
├── admin-ui/                                # spacestorage-admin-ui — CORE (slice 11)
│   └── src/
│       ├── lib.rs                           # mount on admin-http router
│       ├── pages.rs                         # /ui/cluster, /ui/console (maud/askama)
│       ├── map.rs                           # compose 004/006/010/011 + tenant filter
│       ├── console.rs                       # config + browse via 005
│       └── assets/                          # vanilla JS/CSS, no npm
│
├── ingest/                                  # spacestorage-ingest — CORE (slice 11)
│   └── src/
│       ├── lib.rs                           # KafkaIngest, SyslogHandler
│       ├── declaration.rs                   # cluster object + validation
│       ├── kafka.rs                         # consumer group, fetch, commit
│       ├── syslog.rs                        # Handler impl
│       ├── parse_rfc5424.rs
│       ├── parse_rfc3164.rs
│       ├── format.rs                        # raw | json
│       └── record.rs                        # LogRecord → append
│
├── node/                                    # register syslog handler; mount UI routes
├── config/                                  # ingest kafka {}; entrypoint.ingest {}
├── controlplane/                            # KafkaIngest in cluster log
├── admin-proto/ | spacestorage/             # ingest CLI + JSON types
├── release-profile/                         # first binary: 404 / stub; slice 11: full
└── conformance/                             # map lag, tenant filter, kafka ALO, syslog 1:1
```

**Structure Decision**: Split UI and ingest so slice-11 milestones can land syslog/Kafka without chrome, and so Kafka tests do not load templates. Both stay in-process libraries of `spacestoraged`.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| Checked-in vanilla JS assets | Map poll and table browse in the browser | Pure HTML reload misses SC-001; npm SPA is a non-Rust toolchain (constitution I) |
| Custom/pure-Rust Kafka consumer | Offset commit after durable ack; constitution I | `rdkafka` is C; using `008` producer crate cannot consume |
| Kafka in cluster store vs syslog on entrypoint | Clarify Q4 split of tenant vs cluster admin | One config shape would either give tenants listen ports or deny Kafka self-service |

## Constitution Check (post-design)

Re-evaluated after Phase 1: UIs are axum routes on `admin-http`; ingest Kafka is an outbound consumer; syslog is one handler per port; records `append` through `log_stream`/`005`; durable ack before OffsetCommit; overflow drops; series names are additive and distinct from export; first binary gated by `UiIngestSlice11Required`. Static JS is not a Cargo dependency. **Gate result: PASS.**
