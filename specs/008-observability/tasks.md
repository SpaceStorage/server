---
description: "Task list for observability, billing metrics, and logging"
---

# Tasks: Observability, Billing Metrics, and Logging

**Input**: Design documents from `/specs/008-observability/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md), [data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: Requested in [plan.md](plan.md) (unit: omit-label encoder, tenant filter, channel gating, freshness freeze; `crates/conformance`: first-binary `/metrics` 200 + `001` buffer names; slice-9 tenant isolation, Kafka/syslog delivery, primary-down freshness; contract tests on `contracts/fixtures/` and catalog golden snippets). Write failing tests first where listed.

**Scope of this feature**: Own the **in-memory metric registry**, **Prometheus/OTLP exposition**, and **outbound logs**. Other features **record**; this crate **names, stores, filters, and exports**. First binary (`016` slices 1–5): global `GET /metrics` for implemented-path series. Slice 9: full catalog conformance, tenant surfaces, OTel, Kafka/syslog, log channels, aggregation freshness.

**Sibling crates** (wire seams; do not reimplement owner behavior): `crates/node` (`stats.rs`, `admin-http`), `crates/internode` (`MetricsPush` / additive `MetricsSnapshot`), `crates/controlplane`, `crates/tenancy`, `crates/config`, `crates/exec`, `crates/release-profile`, `crates/conformance`.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: [US1], [US2], [US3] on user-story phase tasks only
- Every task includes an exact file path

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: New `spacestorage-observability` crate and workspace wiring per [plan.md](plan.md) Project Structure

- [X] T001 Create `crates/observability/Cargo.toml` (package `spacestorage-observability`, edition 2024, MSRV-compatible) with workspace deps `tokio`, `axum`/`hyper`, `serde`/`serde_json`, `tracing`/`tracing-subscriber`, `bytes`, `parking_lot`, `rustls`; optional features `slice9` / `otel` / `kafka` for OTLP (`opentelemetry` + `opentelemetry-otlp` `http-proto` only) and pure-Rust Kafka (`rskafka` or in-crate Produce) — **forbid** `rdkafka`/`librdkafka`/`openssl`/`prometheus` CounterVec
- [ ] T002 Create `crates/observability/src/lib.rs` that `mod`s `sample`, `encode`, `filter`, `http`, `otel`, `logs`, `kafka`, `syslog`, `aggregation`, `catalog` and exports `Registry` + `Recorder`
- [ ] T003 [P] Add `crates/observability` to workspace `[workspace.members]` in `Cargo.toml` and wire first-binary `release-profile` to link registry+`/metrics` while slice-9/complete-product enables OTLP/Kafka/syslog/tenant in `crates/release-profile`
- [ ] T004 [P] Add empty module stubs `crates/observability/src/{sample,encode,filter,http,otel,logs,kafka,syslog,aggregation,catalog}.rs` so the crate compiles as a library with no new binary

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: In-memory registry, omit-label rules, catalog scaffolding, validation codes, and config grammar every story uses. No user-story work until this phase is complete.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [ ] T005 Implement `MetricName` validation `^[a-zA-Z_:][a-zA-Z0-9_:]*$` and `LabelSet` as `BTreeMap<LabelName, LabelValue>` in `crates/observability/src/sample.rs` with invariants: key **absent** when unknown (FR-023); empty string and sentinels (`unknown`, `-`) **forbidden**; label names `[a-zA-Z_][a-zA-Z0-9_]*`; series identity = `(MetricName, LabelSet)`
- [ ] T006 Implement `Sample` (`counter` \| `gauge` \| `histogram` with `AtomicU64`/`AtomicI64` or histogram `counts[bucket]`/`sum`/`count`) and process-wide `Registry` (map `(name, labels)` → Sample; sample `interval` default **1 s** for derived ratios) in `crates/observability/src/sample.rs` / `crates/observability/src/lib.rs`; recorders only `inc`/`set`/`observe`; encoder iterates under short lock or lock-free snapshot
- [ ] T007 [P] Implement histogram bucket constants in `crates/observability/src/catalog.rs` per R10: duration seconds `0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10, 30, 60`; size bytes `64, 256, 1024, 4096, 16384, 65536, 262144, 1048576, 4194304, 16777216, 67108864`
- [ ] T008 [P] Register HELP/TYPE catalog families (names only; zero-absent until recorded) for system/`001` reserved series in `crates/observability/src/catalog.rs` from [contracts/catalog.md](contracts/catalog.md) and [contracts/metrics.md](contracts/metrics.md): `spacestorage_uptime_seconds`, `node_ready`/`spacestorage_node_ready`, `node_state`/`spacestorage_node_state`, `spacestorage_worker_threads`, buffer usage/limit-hits, freshness pair names
- [ ] T009 Implement Prometheus text 0.0.4 encoder (OpenMetrics 1.0 MAY via `Accept`) in `crates/observability/src/encode.rs`: omit unknown label keys; never emit `""` or `unknown`; `HELP`/`TYPE` once per name; histogram `_bucket`/`_sum`/`_count` with `le` always on buckets; reject inventing labels
- [ ] T010 [P] Export validation codes in `crates/observability/src/lib.rs` (or `error.rs`): `ObservabilitySlice9Required`, `UnknownLabelForbidden`, `MetricsReadDenied`, `ForeignNamespace`, `SinkConfigInvalid`, `KeyMaterialForbidden` per [data-model.md](data-model.md)
- [ ] T011 Implement `metrics { … }` / additive `log { kafka|syslog }` / namespace telemetry grammar and live-reload classes in `crates/config` per [contracts/config-directives.md](contracts/config-directives.md): slow_query default **enabled off**, threshold default **1s**; `audit_log` default **off**; first-binary profile rejecting otel/kafka/syslog/slow_query-on/audit-on/tenant paths with `ObservabilitySlice9Required`; empty Kafka brokers / bad syslog / non-http(s) OTLP → `SinkConfigInvalid`
- [ ] T012 Wire `node` `stats.rs` as a `Recorder` backend registering atomics on `observability::Registry` at boot in `crates/node/src/stats.rs` (SC-004: `/v1/buffers` JSON stays `001`; scrape percent MUST match CLI within one sample interval); do not duplicate buffer counters

**Checkpoint**: `cargo test -p spacestorage-observability` compiles; omit-label unit tests can start. User stories can begin.

---

## Phase 3: User Story 1 - Scrape global node and query metrics (Priority: P1) 🎯 MVP

**Goal**: Global `GET /metrics` on `admin-http` returns **200** Prometheus text for implemented-path series (`001` node/buffer/worker + any reserved names already incremented). Shared-datatype aggregates merge via `006` `MetricsPush`, copy via `MetricsSnapshot`, and expose dedicated freshness gauges — last values remain when primary is down; local series never labelled `stale`.

**Independent Test**: Run a node, execute sample queries/replication, scrape `/metrics`, assert required names/labels; stop aggregator primary and assert freshness series (SC-004, SC-005, SC-011 paths in [quickstart.md](quickstart.md) §1–2, §5).

### Tests for User Story 1 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T013 [P] [US1] Unit-test omit-label encoder in `crates/observability/src/encode.rs` (or `crates/observability/tests/encode_omit.rs`): same metric name with/without `user`; assert illegal `user=""` and `user="unknown"` never appear; `UnknownLabelForbidden` on empty/sentinel insert
- [ ] T014 [P] [US1] Unit-test freshness freeze in `crates/observability/src/aggregation.rs`: when `up=0`, `last_success` and `merged` frozen; no `stale` label on local or merged series
- [ ] T015 [P] [US1] Conformance first-binary scrape in `crates/conformance/tests/metrics_first_binary.rs`: `GET /metrics` → HTTP 200 (not 404); body includes `spacestorage_buffer_usage_bytes`, `spacestorage_buffer_usage_ratio`, `spacestorage_buffer_limit_hits_total`, `node_ready`, `spacestorage_node_ready`, `node_state`, `spacestorage_uptime_seconds`, `spacestorage_worker_threads`; buffer ratio agrees with CLI within one interval (SC-004)
- [ ] T016 [P] [US1] Contract fixture tests in `crates/config/tests/observability_validate.rs` (or conformance): [contracts/fixtures/metrics-block.conf](contracts/fixtures/metrics-block.conf) validates; [contracts/fixtures/invalid/](contracts/fixtures/invalid/) otel-on-first-binary → `ObservabilitySlice9Required`; kafka-empty-brokers → `SinkConfigInvalid`

### Implementation for User Story 1

- [ ] T017 [P] [US1] Implement `AggregationFreshness` (`namespace_id` UUID, `datatype_id` UUID, current `namespace`/`datatype` labels, `up` `0|1`, `last_success` Unix seconds, `merged` sample map) in `crates/observability/src/aggregation.rs` with invariants from [data-model.md](data-model.md) §5 and [contracts/aggregation.md](contracts/aggregation.md)
- [ ] T018 [US1] Merge `MetricsPush` on namespace primary and emit additive `MetricsSnapshot` to secondaries each `cluster.raft.heartbeat` in `crates/observability/src/aggregation.rs` + `crates/internode`; silent replica > `2 * heartbeat` ⇒ `up=0`; expose `spacestorage_shared_aggregation_up{namespace,datatype}` and `spacestorage_shared_aggregation_last_success_timestamp_seconds{namespace,datatype}`; never drop merged series solely because primary is down
- [ ] T019 [US1] Implement axum handlers in `crates/observability/src/http.rs` and mount on `admin-http` per [contracts/admin-http-metrics.md](contracts/admin-http-metrics.md) / [contracts/exposition.md](contracts/exposition.md): `GET /metrics` → 200 Prometheus text (`Content-Type: text/plain; version=0.0.4; charset=utf-8`); auth bearer until `14` then `METRICS_READ`; aliases `node_ready`↔`spacestorage_node_ready` and `node_state`↔`spacestorage_node_state` same value
- [ ] T020 [US1] Register query-processing family names from [contracts/catalog.md](contracts/catalog.md) FR-006 in `crates/observability/src/catalog.rs` and expose `Recorder` hooks so `crates/exec` can increment `spacestorage_query_*` (totals, errors, in-flight, duration/queue/wait histograms, result-size histogram) with labels `kind`, `namespace`, `schema`, `datatype`, `storage_type`, `drive`, `error_type`, `node` omitted when unknown
- [ ] T021 [US1] Ensure missing catalog families are **absent** (not zero) until owners record them; first binary MUST NOT require tenant/OTel/Kafka in `crates/release-profile` / `crates/observability`

**Checkpoint**: First-binary `/metrics` scrapes successfully; buffer series match CLI; freshness unit tests pass; primary-down behavior ready for slice-9 conformance. MVP deliverable.

---

## Phase 4: User Story 2 - Per-namespace metrics and logs (Priority: P1)

**Goal**: Namespace-only scrape and/or OTel push; global admin stream unchanged. Outbound logs to Kafka and/or syslog (global and per-namespace). Default channel = lifecycle + failures only; slow-query and audit channels off until enabled. Tenant sinks never see foreign namespaces; keys never in logs.

**Independent Test**: Two namespaces; enable scrape for one and OTel for the other; compare tenant surfaces vs global; deliver default/slow-query/audit lines to separate sinks (SC-003, SC-007–SC-010; [quickstart.md](quickstart.md) §3–4).

### Tests for User Story 2 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T022 [P] [US2] Unit-test tenant filter in `crates/observability/src/filter.rs`: only series with `namespace=<name>`; 0 foreign-namespace; 0 node-global (no `namespace` label)
- [ ] T023 [P] [US2] Unit-test channel gating in `crates/observability/src/logs.rs`: default on for errors/lifecycle; successful under-threshold queries emit 0 lines; slow_query off → 0 slow lines; audit off → 0 `14` payloads
- [ ] T024 [P] [US2] Conformance tenant isolation in `crates/conformance/tests/metrics_tenant.rs`: `acme` scrape has 0 `namespace="other"` and 0 series without `namespace`; scrape disabled → 404 `metrics_disabled`; unknown ns → 404 `unknown_namespace`; OTel payload series set matches scrape (omit-label included) (SC-003)
- [ ] T025 [P] [US2] Conformance Kafka/syslog delivery in `crates/conformance/tests/logs_sinks.rs`: enable one, the other, or both → same default-channel events on each enabled sink; tenant sink never receives other namespace (incl. audit); key material dropped with `KeyMaterialForbidden` (SC-007–SC-010)

### Implementation for User Story 2

- [ ] T026 [P] [US2] Implement `filter_namespace` in `crates/observability/src/filter.rs` and tenant route `GET /metrics/namespaces/{name}` in `crates/observability/src/http.rs` (slice 9); auth `METRICS_READ` for that namespace; first-binary profile → `ObservabilitySlice9Required` or route absent
- [ ] T027 [US2] Implement optional `handler metrics;` entrypoint (inventory name `metrics`) speaking only `/metrics` and tenant path in `crates/node` / `crates/observability/src/http.rs` per [contracts/admin-http-metrics.md](contracts/admin-http-metrics.md); `disable metrics;` allowed; not mandatory on first binary
- [ ] T028 [US2] Implement OTLP/HTTP protobuf push in `crates/observability/src/otel.rs` (feature-gated): global from `metrics.otel` (interval default **15s**); per-namespace from `NamespaceTelemetry.otel_endpoint`; POST `{endpoint}/v1/metrics`; resource `service.name=spacestorage`, `service.instance.id=<node uuid>`, `service.namespace=spacestorage`; same series set as scrape; failures → `spacestorage_otel_export_errors_total{stream}` + default-channel log; never block recorders
- [ ] T029 [P] [US2] Implement `LogChannel` (`default` on; `slow_query` off; `audit` off) and `LogEvent` fields (`time`, `channel`, `node`, optional `namespace`, `severity` `info|notice|err`, `message`, `fields`, optional `audit`) in `crates/observability/src/logs.rs` per [data-model.md](data-model.md) §6–7; `SinkLayer` on `tracing` target `spacestorage::ops` with `channel=…`
- [ ] T030 [P] [US2] Implement pure-Rust Kafka producer in `crates/observability/src/kafka.rs`: JSON per [contracts/logs.md](contracts/logs.md); key = namespace or `_cluster`; TLS via rustls file refs; at-least-once; no `rdkafka`
- [ ] T031 [P] [US2] Implement RFC 5424 syslog UDP/TCP (octet-counted on TCP) in `crates/observability/src/syslog.rs`: facility `local0`; APP-NAME `spacestorage`; SD-ID `ss@32473` params `ns`, `channel`, `node`; severity from channel (errors=`err`, lifecycle=`info`, slow-query=`notice`, audit=`notice`); RFC 3164 outbound not required
- [ ] T032 [US2] Implement `NamespaceTelemetry` cluster-log apply (scrape bool default false; optional otel/kafka/syslog; slow_query override; `audit_export` default false) on every node in `crates/observability` + `crates/controlplane` / `crates/tenancy`; `007` `private_metrics`/`private_logs` true on first-binary without this record → `ObservabilitySlice9Required`
- [ ] T033 [US2] Wire slow-query (`enabled` default off, threshold default **1s**, live-reloadable) and audit export (`14` content only; tenant sinks namespace-scoped only; cluster audit configure requires `AUDIT_READ`) in `crates/observability/src/logs.rs`; enabling extras MUST NOT disable default channel; overflow of bounded mpsc → drop + `spacestorage_log_export_errors_total` + default-channel log

**Checkpoint**: Tenant scrape/OTel isolate namespaces; Kafka and syslog deliver default/slow/audit correctly; first-binary still rejects slice-9 config.

---

## Phase 5: User Story 3 - Replication, API, durability, datatype, and background-job series (Priority: P2)

**Goal**: Complete product catalog families for replication, communication/API, durability (`db_wal_*`…), LSM/HNSW, background jobs (FR-012 list), and DNS — names owned here; increments by owner crates. Billing keeps namespace+datatype labels; no money formula.

**Independent Test**: Exercise replication, compaction job, WAL-backed write, HNSW search; scrape and assert named series (SC-001, SC-002, SC-006).

### Tests for User Story 3 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T034 [P] [US3] Catalog golden / contract snippets in `crates/observability/tests/catalog_golden.rs`: assert HELP/TYPE and required names from [contracts/catalog.md](contracts/catalog.md) for durability, `db_lsm_*`, `db_hnsw_*`, job, replication, API/comm families; 0 required series renamed
- [ ] T035 [P] [US3] Conformance catalog exercise in `crates/conformance/tests/metrics_catalog.rs` (slice 9): after replication lag, WAL write, LSM/HNSW ops, and a listed job run, assert FR-008–FR-014 series exist with known labels present and unknown labels omitted (SC-001, SC-002, SC-006)

### Implementation for User Story 3

- [ ] T036 [P] [US3] Register communication (FR-007) and API (FR-010) families in `crates/observability/src/catalog.rs` and `Recorder` hooks for `crates` protocol handlers (`002`/`node`): `spacestorage_comm_*`, request/connection totals/errors/duration/active gauges; labels `user`/`application`/`protocol` omitted when unknown
- [ ] T037 [P] [US3] Register replication families (FR-008) in `crates/observability/src/catalog.rs` for owners `004`/`012`: status, lag seconds, last block (omit series if impossible), queues, wait histogram, retries, throughput, failures
- [ ] T038 [P] [US3] Register durability series (FR-014) exactly as intent names in `crates/observability/src/catalog.rs` for owner `013`: `db_wal_bytes_total`, `db_wal_fsync_total`, `db_wal_fsync_duration_seconds`, `db_checkpoint_total`, `db_checkpoint_duration_seconds`, `db_dirty_blocks`, `db_dirty_bytes`, `db_unflushed_bytes`, `db_wal_lag_bytes`, `db_wal_replay_duration_seconds`, `db_wal_recovery_duration_seconds`, `db_recovery_total`, `db_recovery_duration_seconds`, `db_recovery_records_total`
- [ ] T039 [P] [US3] Register datatype series (FR-013) in `crates/observability/src/catalog.rs` for types/LSM/HNSW owners: `db_lsm_memtable_size_bytes`, `db_lsm_sstable_count`, `db_lsm_compaction_total`, `db_lsm_compaction_duration_seconds`, `db_lsm_compaction_pending`, `db_lsm_tombstones`, `db_hnsw_nodes`, `db_hnsw_edges`, `db_hnsw_search_duration_seconds`, `db_hnsw_search_ef`
- [ ] T040 [P] [US3] Register background-job families (FR-011/FR-012) in `crates/observability/src/catalog.rs`: labels `job` name and `status` (`starting`\|`running`\|`completed`\|`failed`|…); series for duration, errors, retries, queue size/duration/errors, queue totals; cover jobs compaction, flush, checkpoint, vacuum, GC, index rebuild, replication, backup, cleanup, TTL expiration, snapshot, data transformation, data migration, data backup, data restore (behavior remains owner features)
- [ ] T041 [US3] Register system remainder (FR-009) including DNS resolution duration and request/error totals, storage read/write/fsync/flush duration/bytes, cache hits/misses/evictions, worker usage percent/busy, leader election totals/errors/duration histogram in `crates/observability/src/catalog.rs`; DNS for exporter sinks may be incremented by this crate per [contracts/metrics.md](contracts/metrics.md)
- [ ] T042 [US3] Document owner→family map in `crates/observability/src/catalog.rs` comments matching [contracts/metrics.md](contracts/metrics.md); ensure billing-relevant usage series always carry `namespace` and `datatype` when known (FR-015); do not define a money formula

**Checkpoint**: Full catalog names registered; conformance can assert series after owner exercise; no renames of `db_*` or reserved `spacestorage_*`.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Operator path, performance budgets, docs, profile gates

- [ ] T043 [P] Align [quickstart.md](quickstart.md) and `docs/examples/` (if present) so an operator scrapes `/metrics` and identifies `node_state`, in-flight queries, and buffer saturation in under 10 minutes (SC-005)
- [ ] T044 [P] Add/confirm unit tests for encode p95 budget documentation note and record-path non-blocking: bounded mpsc overflow drops rather than blocking query workers in `crates/observability/src/logs.rs` / `otel.rs`
- [ ] T045 Ensure `007` private flags + slice-9 gates and `release-profile` first-binary vs complete-product compile flags keep Kafka/OTLP optional deps off slices 1–5 in `crates/release-profile` and `crates/observability/Cargo.toml`
- [ ] T046 [P] Run [quickstart.md](quickstart.md) §0–§5 validation commands against in-process/loopback binary and keep them executable from `crates/conformance`
- [ ] T047 [P] `rustfmt`/`clippy` clean pass on `crates/observability` and new conformance/config tests

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Setup — **BLOCKS** all user stories
- **User Story 1 (Phase 3)**: Depends on Foundational — MVP global scrape + freshness core
- **User Story 2 (Phase 4)**: Depends on Foundational; benefits from US1 encoder/HTTP but independently testable for tenant/logs
- **User Story 3 (Phase 5)**: Depends on Foundational catalog/registry; can proceed in parallel with US2 once registry+encoder exist
- **Polish (Phase 6)**: Depends on desired user stories being complete

### User Story Dependencies

- **User Story 1 (P1)**: After Foundational — no dependency on US2/US3
- **User Story 2 (P1)**: After Foundational — uses encoder/filter; independently testable via tenant/sink conformance
- **User Story 3 (P2)**: After Foundational — catalog registration + owner hooks; independently testable via catalog golden + exercise suite

### Within Each User Story

- Tests (where listed) MUST be written and FAIL before implementation
- Models/registry before services/encoders
- Encoder/HTTP before aggregation exposition
- Core implementation before cross-crate owner wiring
- Story complete before treating the next priority as done

### Parallel Opportunities

- Phase 1: T003–T004 in parallel after T001–T002 skeleton
- Phase 2: T007–T008, T010 in parallel once Sample types exist
- Phase 3: T013–T016 tests in parallel; T017 parallel with early HTTP once Registry exists
- Phase 4: T022–T025 tests in parallel; T026, T029–T031 implementation modules in parallel
- Phase 5: T034–T035 tests and T036–T041 catalog registrations largely parallel
- After Foundational, US1 / US2 / US3 can be staffed in parallel with care on shared `catalog.rs` / `http.rs`

---

## Parallel Example: User Story 1

```bash
# Launch all US1 tests together:
Task: "Unit-test omit-label encoder in crates/observability/src/encode.rs"
Task: "Unit-test freshness freeze in crates/observability/src/aggregation.rs"
Task: "Conformance first-binary scrape in crates/conformance/tests/metrics_first_binary.rs"
Task: "Contract fixture tests in crates/config/tests/observability_validate.rs"

# After tests fail, implement core in parallel where files differ:
Task: "Implement AggregationFreshness in crates/observability/src/aggregation.rs"
Task: "Implement axum handlers in crates/observability/src/http.rs"
```

---

## Parallel Example: User Story 2

```bash
Task: "Unit-test tenant filter in crates/observability/src/filter.rs"
Task: "Unit-test channel gating in crates/observability/src/logs.rs"
Task: "Implement Kafka producer in crates/observability/src/kafka.rs"
Task: "Implement syslog RFC 5424 in crates/observability/src/syslog.rs"
```

---

## Parallel Example: User Story 3

```bash
Task: "Catalog golden tests in crates/observability/tests/catalog_golden.rs"
Task: "Register durability series in crates/observability/src/catalog.rs"
Task: "Register LSM/HNSW series in crates/observability/src/catalog.rs"
Task: "Register background-job families in crates/observability/src/catalog.rs"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup (`crates/observability` member)
2. Complete Phase 2: Foundational (Registry, omit-label encode, config codes, `stats.rs` wire)
3. Complete Phase 3: User Story 1 (global `/metrics` + aggregation freshness)
4. **STOP and VALIDATE**: first-binary scrape + buffer CLI agreement + omit-label/freshness unit tests
5. Demo/ship slices 1–5 metrics surface without tenant/OTel/Kafka

### Incremental Delivery

1. Setup + Foundational → registry ready
2. Add US1 → global scrape MVP
3. Add US2 → tenant isolation + sinks (slice 9)
4. Add US3 → full catalog families
5. Polish → quickstart/SC operator path

### Parallel Team Strategy

1. Team completes Setup + Foundational together
2. Once Foundational is done:
   - Developer A: User Story 1 (HTTP + aggregation)
   - Developer B: User Story 2 (filter, OTel, Kafka/syslog, channels)
   - Developer C: User Story 3 (catalog families + owner hooks)
3. Integrate via shared `Registry` / `catalog.rs` carefully

---

## Notes

- [P] tasks = different files, no dependencies on incomplete work
- [Story] label maps task to US1/US2/US3 for traceability
- Setup/Foundational/Polish phases have **no** story labels
- First binary: sparse catalog OK; missing families absent not zero
- Custom encoder required (no `prometheus` CounterVec) — Complexity Tracking
- Commit after each task or logical group
- Stop at any checkpoint to validate the story independently
- Avoid: inventing label values, `stale` labels on series, `rdkafka`, renaming `db_*` / reserved names
