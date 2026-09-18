# Implementation Plan: Protocol Compatibility Ceiling, Limits, Isolation, and Rolling Upgrade

**Branch**: `015-compatibility-and-limits` | **Date**: 2026-09-18 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/015-compatibility-and-limits/spec.md` (Clarifications, Sessions 2026-09-15 and 2026-09-18 — ceiling not “v1”; stock clients; `SERIALIZABLE` non-goal; spill vs reject; `SNAPSHOT` refuse; PostgreSQL cursor subset; mixed N/N+1 complete-product only; Elasticsearch search/aggregation list)

## Summary

Own the **compatibility ceiling** that constitution V and features `002`/`005`/`012`/`013`/`016` already point at but do not encode as one testable artifact: a machine-readable **MUST / MUST NOT matrix** plus wire versions, a closed **isolation set**, documented **size / connection / admission** limits with hard reject, and the **product-version window** N / N+1.

This feature does **not** re-implement protocol handlers (`002`) or the planner (`005`). It adds `crates/compat` (`spacestorage-compat`) as the single source of truth those crates consult: dialect profile (first binary vs complete product), verb classification, isolation mapping, limit types and defaults, product/internode/disk version window. Handlers return that protocol’s not-supported form **before** IR for MUST NOT verbs. `005` attaches isolation and enforces query-memory / concurrent-query caps. `003` grows a `snapshot_capable` flag. `012`/`013` already refuse out-of-window peers and unknown-major disk; this crate names the window.

**First binary** (`016` slices 1–5): PostgreSQL 3.0 auto-commit DML/DDL + extended/prepared (no `COPY`, no `BEGIN`, no SQL `DECLARE`); Redis RESP2 MUST list on `K/V Store`; size/connection/concurrent-query/query-memory **reject** at cap; same product version on every node; no ES/Cassandra/ClickHouse/S3/WebDAV handlers. **Slice 6**: remaining handlers at this matrix (ES CRUD + MUST search; ES aggregations still not-supported). **Slice 8**: `BEGIN`/`COPY`/SQL cursors, `SNAPSHOT`, spill for sort/hash/aggregation, ES closed aggregations. **Complete-product rolling upgrade**: N/N+1 mix; N refuses N+1 types/formats; N+2 join refused.

## Technical Context

**Language/Version**: Rust 1.87 (edition 2024, MSRV 1.85), same Cargo workspace as `001`–`016`.

**Primary Dependencies**: existing workspace (`tokio`, `async-trait`, `serde`/`serde_json`, `bytes`, `tracing`). No extra protocol crates here — `pgwire`, `redis-protocol`, `axum` stay in `002` handlers. No DataFusion, no OpenSSL, no `*-sys`.

**Storage**: No new user-data format. Limit counters are process-local (connections, in-flight queries, reserved query memory). Spill files remain `{data_dir}/spill/<exec-id>/` owned by `005`. Product version is compiled-in plus optional `cluster { product_version N; }` for tests. On-disk format version stays `013`. Catalog-diff of types across releases stays `003`.

**Testing**: `cargo test`. Unit: matrix classify, isolation map, limit parse, version window. `crates/conformance`: first-binary PG/Redis MUST smoke + MUST NOT (`COPY`/`BEGIN`/`SERIALIZABLE`); oversize/over-connection/over-concurrent/over-memory reject named (SC-004); same-version cluster (SC-005). Slice 6 feature `handlers-complete`: remaining-handler MUST/MUST NOT including ES search and ILM. Slice 8 feature `query-distributed`: SNAPSHOT, cursors, COPY, spill, ES aggregations. Mixed N/N+1 after format versions exist. Contract tests on `contracts/fixtures/`.

**Target Platform**: Linux x86_64/aarch64 servers (primary), macOS for development. Multi-node tests: in-process, loopback, same harness as `011`/`016`.

**Project Type**: Cargo workspace extension — **one new library crate** `crates/compat`. No new binary. Wired from handlers (`002`), `exec` (`005`), `types` (`003` flag), `internode`/`storage` (`012`/`013` window), `config`/`node`/`conformance`. `release-profile`: first-binary dialect; `handlers-complete` slice 6; `query-distributed` slice 8 (already gated in `005`).

**Performance Goals**: Verb classify < 1 µs. Size/connection reject at accept/parse without buffering the oversize body past `max_value` + 1 chunk. Handshake mismatch still < 1 s (`002` SC-008). Admission reject < 1 ms (inherited `005`). Stock-client MUST smokes unchanged in order vs `002`/`016` when under limits.

**Constraints**: Matrix is the complete-product ceiling, never branded “v1”. First binary is a **subset**, not a second product. Stock **clients** on MUST verbs; unmodified **applications** that need MUST NOT are out of scope. `SERIALIZABLE` refused. CPU hard isolation not claimed. Spill MUST NOT replace concurrent-query admission. Unbound `CLUSTER_ADMIN` on Redis/S3/WebDAV/ES stays `014`. Numeric defaults documented here; operators MAY raise within bounds. Mixed-version tests MUST NOT be required for slices 1–5.

**Scale/Scope**: 8 handlers / 7 protocols; 2 dialect profiles; ~7 size/admission knobs; isolation set of 2 + 1 refuse; version window width 2. Roughly: matrix+profiles (~2 k), limits+config (~1.5 k), isolation+version (~1 k), handler/exec wiring (~2 k), conformance (~3 k); ≈ 9–12 k lines including tests.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Gate | Status |
|---|-----------|------|--------|
| I | Rust-Only Stack | `compat` is pure Rust; no new C-backed protocol stack | PASS |
| II | Fully Asynchronous Tokio Runtime | Classify/limit checks are non-blocking. Spill stays `005` `spawn_blocking` | PASS |
| III | Single-Process Multithreaded Monolith | Library in `spacestoraged`; no compatibility sidecar | PASS |
| IV | Type-Driven Multiparadigm | No new datatype. `snapshot_capable` is a `003` descriptor flag this feature consumes | PASS |
| V | Protocol Compatibility on Distinct Ports | **This feature is the ceiling of the principle.** Eight handlers remain `002`; first binary still PG+Redis (`016`). No SpaceStorage-native client protocol | PASS |
| VI | Every Node Is a Request Coordinator | Limits and matrix apply on the receiving node; no mandatory proxy | PASS |
| VII | Label-Based Planetary Placement | Unchanged | PASS |
| VIII | Cassandra-Style Quorum | Consistency is not SQL isolation (FR-007). Quorum arithmetic stays `012`/`004` | PASS |
| IX | Multi-Tenant Namespaces | Concurrent-query cap per namespace; connections per principal; quotas remain `007` | PASS |
| X | Observability as a Product Surface | Increments `limit_exceeded` / admission counters with `08` label names; no required series renamed | PASS |
| XI | Documented, Expandable Configuration | `limits { }` + documented defaults + starters; matrix is data not a closed language | PASS |
| XII | Raft Controller Elections and Local Restore | No new Raft. Version window on join/format; restore still `013` | PASS |
| XIII | Security Defaults for Data and Roles | Unchanged (`014`). Connection-per-principal is a limit, not a role | PASS |
| Arch. Contracts | Adapter ≠ type system | Handlers still lower MUST verbs to `LogicalRequest`; MUST NOT never becomes IR | PASS |
| Observability Contract | No `08` series renamed | Additive limit/admission figures only | PASS |

**Gate result (pre-research)**: PASS. Sequenced deviations (005 spill default for complete product; 003 `snapshot_capable`; 002 PG “cursors beyond portals” superseded for complete product) are in Complexity Tracking.

## Project Structure

### Documentation (this feature)

```text
specs/015-compatibility-and-limits/
├── plan.md
├── research.md                     # Phase 0: R1–R13
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── dialect-profiles.md         # first binary vs complete product
│   ├── matrix.md                   # MUST / MUST NOT per protocol
│   ├── wire-versions.md
│   ├── isolation.md
│   ├── limits.md                   # size, connection, admission policy
│   ├── postgresql-cursors.md       # FR-014
│   ├── elasticsearch-search.md     # FR-015
│   ├── copy.md                     # complete-product COPY formats
│   ├── version-window.md           # N/N+1 product, internodes, disk
│   ├── config-directives.md
│   ├── metrics.md
│   └── fixtures/
│       ├── README.md
│       ├── first-binary.conf
│       ├── limits-raised.conf
│       └── invalid/
├── checklists/requirements.md
└── tasks.md                        # Phase 2 (/speckit-tasks — NOT created here)
```

### Source Code (repository root)

```text
crates/
├── compat/                                  # spacestorage-compat — NEW
│   └── src/
│       ├── lib.rs                           # DialectProfile, classify, IsolationSet, Limits
│       ├── matrix.rs                        # per-protocol MUST/MUST NOT tables
│       ├── wire.rs                          # handshake versions
│       ├── isolation.rs                     # READ COMMITTED, SNAPSHOT, SERIALIZABLE refuse
│       ├── limits.rs                        # defaults, LimitKind, check_size
│       ├── version.rs                       # ProductVersion window
│       └── profile.rs                       # FirstBinary | CompleteProduct | HandlersComplete
│
├── protocol-core/ | handler-postgresql/ | handler-redis/
├── handler-cassandra/ | handler-elasticsearch/ | handler-clickhouse/
├── handler-s3/ | handler-webdav/            # classify() before IR; slice 6
├── exec/                                    # consume Limits + IsolationSet; spill policy
├── types/                                   # TypeDescriptor.snapshot_capable
├── internode/ | storage/                    # version window (012/013)
├── config/ | node/                          # limits { }; product_version
├── release-profile/                         # first-binary dialect
└── conformance/                             # SC-001–SC-005
```

**Structure Decision**: New `compat` crate so `002` handlers and `005` exec share one table without `exec` depending on a handler crate or `protocol-core` growing limit/version policy. Spill files and planner stages stay in `exec`. Type snapshot flags stay in `types`. Internode frames and WAL format stay in `012`/`013`; they **import** the window predicate.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| Extra crate `compat` besides `protocol-core` | Matrix + limits + version window are consumed by handlers, exec, internodes, and types | Stuffing `protocol-core` would pull internodes/storage into the driver toolkit; duplicating tables would drift |
| Revise `005` `query.spill` default to `on` for slice 8 / complete product | Spec FR-010: complete product MUST spill for sort/hash/aggregation | Leaving `spill off` forever would make ClickHouse GROUP BY / SQL sorts fail at the memory cap |
| Add `snapshot_capable` on `003` TypeDescriptor | Spec FR-006: this feature owns refuse; `03` marks types | Hardcoding only `Relational Table` in `compat` would hide new snapshot-capable types |
| Complete-product SQL `DECLARE` supersedes `002` “cursors beyond portals → not-supported” | Clarify 2026-09-18 Q3 / FR-014 | Keeping the `002` line would contradict the documented holdable subset |
| Mixed-version suite not in first binary | Clarify Q4; `016` defers rolling upgrade | Requiring two binaries in slices 1–5 blocks WAL format work (`013`) |

## Constitution Check (post-design)

Re-evaluated after Phase 1: matrix lives in `compat` and is consulted before IR; first-binary dialect is a subset; isolation refuse has no silent downgrade; size/connection/admission hard-reject; spill is slice 8; N/N+1 is complete-product; ES search list is here, aggregations slice 8; no second engine; no native client protocol; CPU isolation not claimed. **Gate result: PASS.**
