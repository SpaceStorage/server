---
description: "Task list for cluster admin UIs and log ingest"
---

# Tasks: Cluster Admin UIs and Log Ingest

**Input**: Design documents from `/specs/009-admin-ui-ingest/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md), [data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: Requested. Plan Testing section + Independent Tests (US1–US3) + SC-001–SC-007 + contract fixtures under [contracts/fixtures/](contracts/fixtures/). Unit: RFC 5424/3164 parse, raw vs JSON Kafka mapping, map tenant filter, buffer drop. Conformance: three-node lag map; namespace-only map; Kafka at-least-once; syslog 1:1 ports; WRITE-only ingest refused. Write failing tests first where listed.

**Scope of this feature**: Two library crates `crates/admin-ui` (`spacestorage-admin-ui`) and `crates/ingest` (`spacestorage-ingest`) mounted into `spacestoraged` (no new binary). UIs on existing `admin-http`. Kafka = declared outbound consumer group; syslog = `001` handler, 1:1 listen port. First-binary profile (`016` slices 1–5) MUST NOT require UI/ingest (`UiIngestSlice11Required`). Do not share series or code paths with `008` export.

**Sibling crates** (seams; do not reimplement): `crates/node`, `crates/config`, `crates/controlplane`, `crates/admin-proto` / `crates/spacestorage`, `crates/release-profile`, `crates/conformance`, plus topology/`005`/`003`/`013`/`014` from sibling specs.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: [US1], [US2], [US3] on user-story phase tasks only
- Every task includes an exact file path

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Workspace members, crate skeletons, Cargo features for slice 11, fixture/docs trees

- [ ] T001 Create `crates/admin-ui/Cargo.toml` (package `spacestorage-admin-ui`, edition 2024) and `crates/admin-ui/src/lib.rs` that `mod`s `pages`, `map`, `console`, `assets` per [plan.md](plan.md) project structure
- [ ] T002 [P] Create `crates/ingest/Cargo.toml` (package `spacestorage-ingest`, edition 2024) and `crates/ingest/src/lib.rs` that `mod`s `declaration`, `kafka`, `syslog`, `parse_rfc5424`, `parse_rfc3164`, `format`, `record`
- [ ] T003 Add `crates/admin-ui` and `crates/ingest` to workspace `[workspace.members]` in `Cargo.toml`; wire optional slice-11 features in `crates/release-profile` / `crates/spacestoraged` so default/`first-binary` does not link Kafka UI/ingest deps
- [ ] T004 [P] Add pure-Rust UI template dep (`maud` or `askama`) to `crates/admin-ui/Cargo.toml` and create empty asset dirs `crates/admin-ui/src/assets/` for checked-in vanilla JS/CSS (no `package.json`, no npm)
- [ ] T005 [P] Add pure-Rust Kafka client dep (`rskafka` and/or `kafka-protocol`) + `rustls`/`tokio-rustls` to `crates/ingest/Cargo.toml`; **forbid** `rdkafka`/`librdkafka` and OpenSSL
- [ ] T006 [P] Copy starter fixtures from [contracts/fixtures/](contracts/fixtures/) into `docs/examples/ingest/` (`ingest-kafka.conf`, `ingest-syslog.conf`) and keep invalid fixtures referenced from conformance only

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Shared types, config grammar, authz/error codes, first-binary gate, buffers, metrics registration, router/handler registration seams. MUST complete before any user story.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [ ] T007 Implement `ClusterMapView` / `KafkaIngest` / `SyslogIngestBind` / `LogRecord` Rust shapes in `crates/admin-ui/src/map.rs` (view DTOs) and `crates/ingest/src/declaration.rs` + `crates/ingest/src/record.rs` per [data-model.md](data-model.md): `KafkaIngest.id` UUID immutable; `name` unique IDENT; `format` `raw` \| `json` default `raw`; `type` default `log_stream`; `SyslogIngestBind` requires `namespace` + `container`; `ClusterMapView.scope` `cluster` \| `namespace`
- [ ] T008 [P] Export validation/error codes in `crates/ingest/src/lib.rs` (or `error.rs`): `ingest_missing_target`, `ingest_unknown_type`, `ingest_forbidden`, `ingest_write_only`, `ingest_kafka_no_brokers`, `ingest_kafka_no_topic`, `ingest_kafka_no_group`, `ingest_cert_inline`, `ingest_syslog_cluster_only`, `UiIngestSlice11Required`
- [ ] T009 [P] Parse `entrypoint.ingest { namespace; container; type?; }` child and optional bootstrap `ingest kafka NAME { … }` in `crates/config` per [config-directives.md](contracts/config-directives.md); `type` default `log_stream`; `format` default `raw`; reload class live for Kafka, restart for syslog bind
- [ ] T010 On first-binary profile, reject configs that enable `handler syslog`, `ingest kafka`, or `/ui/*` with `UiIngestSlice11Required` (exit 2) in `crates/config/src/validate.rs` and `crates/release-profile` — lock fixtures [syslog-on-first-binary.conf](contracts/fixtures/invalid/syslog-on-first-binary.conf) and [kafka-on-first-binary.conf](contracts/fixtures/invalid/kafka-on-first-binary.conf)
- [ ] T011 [P] Register buffers `ingest.syslog.recv` (default 16 MiB, range 1 MiB–1 GiB, policy **reject**) and `ingest.kafka.decode` (default 32 MiB, range 1 MiB–1 GiB, policy **reject**) in `crates/config` / node buffer inventory per [config-directives.md](contracts/config-directives.md); never `wait` on a Tokio worker (R7)
- [ ] T012 [P] Register additive metric series in `crates/ingest` (exposition via `008`): `spacestorage_ingest_records_total`, `spacestorage_ingest_parse_errors_total`, `spacestorage_ingest_dropped_total`, `spacestorage_ingest_kafka_offset_commits_total` with labels from [metrics.md](contracts/metrics.md); do **not** increment `spacestorage_log_export_*`
- [ ] T013 Add mount/register seams: `crates/admin-ui` router mount helper for `admin-http` in `crates/admin-ui/src/lib.rs`; `SyslogHandler` + `KafkaIngest` registration hooks in `crates/ingest/src/lib.rs`; call sites in `crates/node` that are no-op/404 under first-binary and full under slice 11
- [ ] T014 [P] Implement interim UI auth: bearer = `001` admin token until `014`; map/console must call the same principal/permission checks (no cookie-only privilege) in `crates/admin-ui/src/lib.rs` per R12

**Checkpoint**: Workspace builds with `admin-ui`/`ingest` behind slice-11 features; first-binary validate fails on UI/ingest fixtures with `UiIngestSlice11Required`; shared types and codes compile. User stories can start.

---

## Phase 3: User Story 1 - Cerebro-like cluster map (Priority: P1) 🎯 MVP

**Goal**: Serve Cerebro-like cluster map HTML + JSON from `admin-http`. Map is a filter over `004`/`006`/`010`/`011`, not a private topology. `CLUSTER_ADMIN` / cluster `METRICS_READ` → full map; `NAMESPACE_ADMIN` → namespace-only; else 403.

**Independent Test**: Three-node replicated container with induced lag; open UI as cluster admin → nodes/shards/lag match topology; as `NAMESPACE_ADMIN` → only that tenant; as principal with no map rights → refused. Operator finds lag in < 2 min (SC-001).

### Tests for User Story 1 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T015 [P] [US1] Add unit tests for tenant filter in `crates/admin-ui/src/map.rs` (or `crates/admin-ui/tests/map_filter.rs`): `scope=namespace` ⇒ 0 foreign-namespace containers; node list only hosts of those containers; neither map rights nor `NAMESPACE_ADMIN` ⇒ no body / forbidden
- [ ] T016 [P] [US1] Add conformance test in `crates/conformance/tests/admin_ui_map.rs`: three-node lag fixture; `GET /v1/cluster/map` as cluster admin shows lagging replica matching topology describe; `NAMESPACE_ADMIN` on `acme` sees `scope=namespace` and 0 other tenants; unprivileged principal → 403
- [ ] T017 [P] [US1] Add conformance check that when a `010` migration job is present, `migrations[]` exposes `id`, `status`, `bytes_copied`, `bytes_remaining` in `crates/conformance/tests/admin_ui_map.rs`

### Implementation for User Story 1

- [ ] T018 [P] [US1] Implement `GET /v1/cluster/map` composer in `crates/admin-ui/src/map.rs`: topology (`004`), membership (`011`), replica health/lag (`004`), migration progress (`010` when present); MUST NOT mint replica ids; optional `?namespace=` with authz per [ui-http.md](contracts/ui-http.md)
- [ ] T019 [US1] Enforce map rights in `crates/admin-ui/src/map.rs`: `CLUSTER_ADMIN` or cluster-scoped `METRICS_READ` → `scope=cluster`; `NAMESPACE_ADMIN` only → `scope=namespace`; other → HTTP 403, no body (FR-002, clarify Q1)
- [ ] T020 [P] [US1] Implement `GET /ui/cluster` HTML page in `crates/admin-ui/src/pages.rs` (maud/askama) and poll JS in `crates/admin-ui/src/assets/cluster-map.js` default interval **2 s** (`?interval=` 1–10); serve assets under `/ui/assets/*`
- [ ] T021 [US1] Mount US1 routes on `admin-http` in `crates/node` (via `crates/admin-ui/src/lib.rs`): `/ui/cluster`, `/ui/assets/*`, `/v1/cluster/map`; first-binary → 404 `{ "code": "UiIngestSlice11Required" }`
- [ ] T022 [US1] Add `spacestorage ui` CLI hint in `crates/spacestorage` / `crates/admin-proto` printing `admin-http` base URL + `/ui/cluster` and `/ui/console` paths per [cli.md](contracts/cli.md)

**Checkpoint**: Cluster map HTML/JSON works with role-scoped visibility; lag fixture meets SC-001; unprivileged → 403. MVP demoable without console or ingest.

---

## Phase 4: User Story 2 - Kibana-like configure and browse (Priority: P1)

**Goal**: Namespace/cluster console on `admin-http`. Config and browse go through `005` with application `spacestorage-ui`. Tenant cannot change cluster-global settings. Respect `015` result size limits (paginate).

**Independent Test**: Tenant creates a relational table and a document store through the UI, browses rows/documents, attempts a cluster-global setting → refused. Queries appear on shared execution path (SC-006).

### Tests for User Story 2 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T023 [P] [US2] Add conformance tests in `crates/conformance/tests/admin_ui_console.rs`: `NAMESPACE_ADMIN` on `acme` can `POST /v1/console/containers` + insert + browse; `POST /v1/console/config` cluster-global → 403 (SC-003); query spans carry `application=spacestorage-ui`
- [ ] T024 [P] [US2] Add unit/integration test that oversized browse respects `015` pagination / 413 rather than buffering an entire object store in `crates/admin-ui/tests/console_limits.rs` (or conformance)

### Implementation for User Story 2

- [ ] T025 [P] [US2] Implement `POST /v1/console/query` in `crates/admin-ui/src/console.rs`: body is a `005` query; MUST execute in `005` (not a UI-private engine); `READ` on named containers; application name `spacestorage-ui`; paginate per `015`
- [ ] T026 [P] [US2] Implement `POST /v1/console/config` in `crates/admin-ui/src/console.rs`: `CLUSTER_ADMIN` for cluster settings; `NAMESPACE_ADMIN` for that namespace only; cross-scope → 403 `forbidden`
- [ ] T027 [P] [US2] Implement `POST /v1/console/containers` in `crates/admin-ui/src/console.rs`: requires `CREATE` + `NAMESPACE_ADMIN` (or `CLUSTER_ADMIN`) → 201
- [ ] T028 [US2] Implement `GET /ui/console` and `GET /ui` (302 → `/ui/console`) in `crates/admin-ui/src/pages.rs` plus browse JS/CSS in `crates/admin-ui/src/assets/console.js` / `console.css`; mount routes in `crates/admin-ui/src/lib.rs` / `crates/node`
- [ ] T029 [US2] Ensure console mutations use the same `014` verbs as CLI (no private privilege model) in `crates/admin-ui/src/console.rs` (FR-005)

**Checkpoint**: Tenant console create→insert→browse works under 15 min with starter docs (SC-002); 100% of cluster-global attempts by namespace-only principal refused (SC-003); browses on `005` (SC-006).

---

## Phase 5: User Story 3 - Kafka and syslog ingest (Priority: P2)

**Goal**: Declare Kafka consumers (cluster store, live-applied) and syslog binds (node entrypoint, 1:1 port). Parse → `log_stream.append` via `005` as `spacestorage-ingest`. Kafka OffsetCommit only after durable ack. Default payload raw UTF-8; optional JSON. RFC 5424 preferred / 3164 accepted. Authz: Kafka = `NAMESPACE_ADMIN`+`WRITE` or `CLUSTER_ADMIN`; syslog bind = `CLUSTER_ADMIN` only; `WRITE` alone refused.

**Independent Test**: Declare log-stream container; send Kafka (raw + JSON) and syslog; read back; kill node mid-ingest → at-least-once (duplicates OK, no silent loss of acked messages). WRITE-only declare refused.

### Tests for User Story 3 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T030 [P] [US3] Add unit tests for RFC 5424 and RFC 3164 parsers in `crates/ingest/src/parse_rfc5424.rs` and `crates/ingest/src/parse_rfc3164.rs`; malformed → parse error, listener not stalled
- [ ] T031 [P] [US3] Add unit tests for payload mapping in `crates/ingest/src/format.rs`: raw invalid UTF-8 → parse failure; JSON non-object root → parse failure; raw success stores message + kafka sidecar fields per [log-record.md](contracts/log-record.md)
- [ ] T032 [P] [US3] Add unit tests for buffer overflow reject (drop + `spacestorage_ingest_dropped_total{reason="buffer_full"}`, no worker wait) in `crates/ingest/tests/buffer_drop.rs`
- [ ] T033 [P] [US3] Add conformance in `crates/conformance/tests/ingest_kafka.rs`: in-process pure-Rust fake broker; durable ack then OffsetCommit; kill after ack before commit → duplicates MAY appear, acked readable (SC-004); WRITE-only declare → 403 `ingest_write_only`
- [ ] T034 [P] [US3] Add conformance in `crates/conformance/tests/ingest_syslog.rs`: two syslog entrypoints → traffic to one port NEVER stored in the other container; RFC 5424 + 3164 stored; [syslog-duplicate-port.conf](contracts/fixtures/invalid/syslog-duplicate-port.conf) → `entrypoint_duplicate_address`; [syslog-missing-target.conf](contracts/fixtures/invalid/syslog-missing-target.conf) → `ingest_missing_target`; non-`CLUSTER_ADMIN` bind → 403
- [ ] T035 [P] [US3] Add config validate tests locking [kafka-empty-brokers.conf](contracts/fixtures/invalid/kafka-empty-brokers.conf) → `ingest_kafka_no_brokers` in `crates/config/tests/ingest_validate.rs`

### Implementation for User Story 3

- [ ] T036 [P] [US3] Implement `KafkaIngest` cluster object CRUD: `GET/POST /v1/ingest/kafka`, `DELETE /v1/ingest/kafka/{id}` in `crates/ingest/src/declaration.rs` + admin routes; persist in `006` cluster log; live-apply start/stop on every `ready` node; authz `NAMESPACE_ADMIN`+`WRITE` or `CLUSTER_ADMIN`; `WRITE` alone → 403 `ingest_write_only`; missing target → 422 `ingest_missing_target`
- [ ] T037 [US3] Implement Kafka consumer group in `crates/ingest/src/kafka.rs` per [kafka-consumer.md](contracts/kafka-consumer.md): all `ready` members join `group`; Fetch → decode → `log_stream.append` via `005` as `spacestorage-ingest` → wait counted durable ack (`013`/`004`) → **then** OffsetCommit; parse failure skip + metric, no commit for that offset, partition continues; buffer full → no enqueue, no commit, retry
- [ ] T038 [P] [US3] Implement `format` mapping in `crates/ingest/src/format.rs` + `record.rs`: default `raw` (UTF-8 value → `message`; key/broker ts/topic/partition/offset sidecars); `json` maps object keys onto Log Stream fields, unknown → `attrs`; invalid UTF-8 / non-object JSON → parse failure codes/metrics
- [ ] T039 [P] [US3] Implement `SyslogHandler` in `crates/ingest/src/syslog.rs`: UDP+TCP on plaintext port; TLS = TCP only; child `ingest { namespace; container; type? }` mandatory; write **only** to declared target; HOSTNAME/APP-NAME/facility stored, never used as routing key (FR-008)
- [ ] T040 [US3] Wire RFC parse chain in `crates/ingest/src/syslog.rs`: prefer 5424 (octet-counted TCP / datagram UDP), fallback 3164, else skip + `spacestorage_ingest_parse_errors_total`; overflow reject per R7; drain stops new accepts (`001`)
- [ ] T041 [US3] Register `handler syslog` in `crates/node` handler inventory for slice 11 only; require `CLUSTER_ADMIN` to distribute/change syslog entrypoint config; `NAMESPACE_ADMIN` admin-API bind attempt → 403 `ingest_syslog_cluster_only`
- [ ] T042 [P] [US3] Add CLI in `crates/spacestorage`: `ingest kafka list|add|delete` and `ingest syslog` (prints that bind is node entrypoint config + starter fixture) per [cli.md](contracts/cli.md); forbidden → exit 3; `UiIngestSlice11Required` → exit 2
- [ ] T043 [US3] On apply of Kafka declaration / syslog handler start, refuse to run without namespace+container (`ingest_missing_target`) in `crates/ingest/src/declaration.rs` and `crates/ingest/src/syslog.rs` (US3.4)
- [ ] T044 [US3] Ensure ingest path does not share producer/export code with `008` in `crates/ingest` (FR-011); application label `spacestorage-ingest` only

**Checkpoint**: Kafka ALO + syslog 1:1 + authz refusals meet SC-004, SC-005, SC-007. Exactly-once MUST NOT be claimed (FR-009).

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Docs alignment, first-binary gating hardening, quickstart validation, lint

- [ ] T045 [P] Align [quickstart.md](quickstart.md) steps 0–4 with live CLI/HTTP paths and fixture paths under `docs/examples/ingest/` and `specs/009-admin-ui-ingest/contracts/fixtures/`
- [ ] T046 [P] Confirm `016` milestone / release-profile documents slice 11 deferral (`UisIngest`) and first-binary absence of UI/ingest without deleting this feature’s stories
- [ ] T047 Run [quickstart.md](quickstart.md) validation end-to-end on slice-11 profile (map lag, console refuse, Kafka raw/JSON, syslog 5424/3164, first-binary refuse) and keep commands executable from `crates/conformance`
- [ ] T048 [P] `rustfmt`/`clippy` clean pass on `crates/admin-ui`, `crates/ingest`, and new conformance tests
- [ ] T049 [P] Document operator poll interval and map rights matrix in `docs/examples/ingest/README.md` (or extend existing admin-http docs) without inventing a second privilege vocabulary

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Setup — **BLOCKS** all user stories
- **User Story 1 (Phase 3)**: After Foundational — no dependency on US2/US3 — 🎯 MVP
- **User Story 2 (Phase 4)**: After Foundational — independently testable; shares `admin-ui` mount with US1 but not ingest
- **User Story 3 (Phase 5)**: After Foundational — independently testable without UI chrome (ingest crate alone)
- **Polish (Phase 6)**: After desired user stories are complete

### User Story Dependencies

- **User Story 1 (P1)**: After Phase 2 only — MVP
- **User Story 2 (P1)**: After Phase 2 only — may reuse auth mount from US1 but remains independently testable via `/v1/console/*`
- **User Story 3 (P2)**: After Phase 2 only — does not require US1/US2 HTML; CLI/API + handlers suffice

### Within Each User Story

- Tests (listed) MUST be written and FAIL before implementation
- Models/DTOs before services/handlers
- Services before HTTP/CLI surfaces
- Core path before crash/rebalance edge cases
- Story complete before raising priority of the next

### Parallel Opportunities

- T001∥T002, T004∥T005∥T006 in Setup
- T008∥T009∥T011∥T012∥T014 in Foundational (after T007 shapes exist where needed)
- US1 tests T015–T017 in parallel; US1 impl T018∥T020 after tests
- US2 tests T023∥T024; impl T025∥T026∥T027 in parallel
- US3 tests T030–T035 in parallel; impl T036∥T038∥T039∥T042 in parallel after tests; T037 depends on T036+T038; T040 depends on T039
- After Foundational, US1 / US2 / US3 can proceed in parallel by different owners (`admin-ui` vs `ingest`)

---

## Parallel Example: User Story 1

```bash
# Tests in parallel:
Task: "Unit tests for tenant filter in crates/admin-ui/src/map.rs"
Task: "Conformance three-node lag map in crates/conformance/tests/admin_ui_map.rs"
Task: "Migration progress migrations[] check in crates/conformance/tests/admin_ui_map.rs"

# Then implementation (map composer ∥ HTML/JS):
Task: "Implement GET /v1/cluster/map in crates/admin-ui/src/map.rs"
Task: "Implement GET /ui/cluster + poll JS in crates/admin-ui/src/pages.rs and assets/"
```

## Parallel Example: User Story 3

```bash
# Parser/format/buffer tests in parallel:
Task: "RFC 5424/3164 unit tests in crates/ingest/src/parse_rfc5424.rs and parse_rfc3164.rs"
Task: "Raw/JSON format unit tests in crates/ingest/src/format.rs"
Task: "Buffer drop unit tests in crates/ingest/tests/buffer_drop.rs"
Task: "Kafka ALO conformance in crates/conformance/tests/ingest_kafka.rs"
Task: "Syslog 1:1 conformance in crates/conformance/tests/ingest_syslog.rs"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL)
3. Complete Phase 3: User Story 1 (cluster map)
4. **STOP and VALIDATE**: Independent Test + SC-001 / map refusals
5. Demo map without console or ingest

### Incremental Delivery

1. Setup + Foundational → foundation ready
2. Add US1 (map) → validate → demo (MVP)
3. Add US2 (console) → validate SC-002/SC-003/SC-006
4. Add US3 (Kafka + syslog) → validate SC-004/SC-005/SC-007
5. Polish / quickstart

### Parallel Team Strategy

1. Team completes Setup + Foundational together
2. Then:
   - Developer A: User Story 1 (`crates/admin-ui` map)
   - Developer B: User Story 2 (`crates/admin-ui` console)
   - Developer C: User Story 3 (`crates/ingest`)
3. Integrate on shared `admin-http` / node registration seams

---

## Notes

- [P] = different files, no incomplete-task dependencies
- [US1]/[US2]/[US3] map to spec user stories
- First binary MUST keep UI/ingest compiled out or gated (`UiIngestSlice11Required`)
- Exactly-once ingest is out of scope (FR-009)
- Do not reuse `008` export metric series or producer paths for ingest
- Commit after each task or logical group; stop at checkpoints to validate independently
