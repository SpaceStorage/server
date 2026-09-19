---
description: "Task list for wire protocols and datatype-aware drivers"
---

# Tasks: Wire Protocols and Datatype-Aware Drivers

**Input**: Design documents from `/specs/002-protocol-drivers/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md), [data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: Requested. Plan Technical Context + SC-001–SC-011 + `crates/conformance/` stock-client suite. Write failing tests first where a Tests subsection appears. Unit tests per seam crate are part of the foundational/story deliverables.

**Scope of this feature**: Eight client-facing handlers (`postgresql`, `cassandra`, `redis`, `elasticsearch`, `clickhouse`, `clickhouse-http`, `s3`, `webdav`) as thin adapters over shared seams (`spacestorage-types`, `spacestorage-exec`, `spacestorage-auth`) and `protocol-core`. Interim in-memory types + `LocalEngine` + `users_file` authenticator ship here; `03`/`05`/`07` replace them behind the same traits. No new binaries.

**Implementation order** (from plan): seams + `protocol-core` → `redis` → `postgresql` → HTTP handlers (`elasticsearch`, `s3`, `webdav`) → `cassandra` → `clickhouse` (native + HTTP); conformance grows with each handler.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: [US1]…[US6] on user-story phase tasks only
- Every task includes an exact file path

## Path Conventions

- Workspace root: `Cargo.toml`, `crates/`, `docs/`
- Feature docs: `specs/002-protocol-drivers/`

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Cargo workspace members, crate skeletons, example config layout

- [ ] T001 Create `crates/types/Cargo.toml` (package `spacestorage-types`, edition 2024) and `crates/types/src/lib.rs` module tree for `datatype`, `canonical`, `ident`, `catalog`, `interim`
- [ ] T002 [P] Create `crates/exec/Cargo.toml` (package `spacestorage-exec`) and `crates/exec/src/lib.rs` module tree for `request`, `options`, `record`, `local`
- [ ] T003 [P] Create `crates/auth/Cargo.toml` (package `spacestorage-auth`) and `crates/auth/src/lib.rs` module tree for `users_file`, `scram`, `sigv4`, `digest`
- [ ] T004 [P] Create `crates/protocol-core/Cargo.toml` (package `spacestorage-protocol-core`) and `crates/protocol-core/src/lib.rs` module tree for `session`, `options`, `signature`, `namespace`, `errors`, `http`, `stats`, `escape`
- [ ] T005 [P] Create handler crate skeletons `crates/handler-{postgresql,cassandra,redis,elasticsearch,clickhouse,s3,webdav}/Cargo.toml` + `src/lib.rs` (ClickHouse crate hosts both `clickhouse` and `clickhouse-http` handlers)
- [ ] T006 [P] Create `crates/conformance/Cargo.toml` (package `spacestorage-conformance`, test-only) and `crates/conformance/tests/harness/mod.rs` stub
- [ ] T007 Add all Phase-1 crates to `[workspace.members]` in `Cargo.toml` with pure-Rust dependencies from plan Technical Context (`pgwire`, `sqlparser`, `cassandra-protocol`, `redis-protocol`, `axum`, `hyper`, `quick-xml`, `hmac`/`sha2`/`md-5`, `lz4_flex`, `flate2`, `serde`/`serde_json`, `ryu`, `tokio`, `async-trait`, `tracing`, `uuid`, `chrono`); forbid `*-sys` in server crates
- [ ] T008 [P] Copy `specs/002-protocol-drivers/contracts/fixtures/node-all-protocols.conf` to `docs/examples/node-all-protocols.conf` and `users.example` to `docs/examples/users.example`

**Checkpoint**: `cargo check -p spacestorage-types -p spacestorage-exec -p spacestorage-auth -p spacestorage-protocol-core` compiles empty crates

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Shared seams, config blocks, and driver toolkit every story needs. No user-story handler work until this phase is complete.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [ ] T009 Implement `NamespaceName` (1–63 chars, `[a-z0-9_-]`), `SchemaName` (default `"public"`), `ContainerName` (1–255 chars, no `/` or NUL), and `ContainerRef` in `crates/types/src/ident.rs`
- [ ] T010 [P] Implement `CanonicalValue`, `Envelope`, `to_canonical_bytes` / `from_canonical_bytes` (RFC 8785-style sorted keys, shortest floats, base64url bytes, media type `application/vnd.spacestorage.canonical+json; v=1`) in `crates/types/src/canonical.rs`
- [ ] T011 [P] Implement `TypeRegistry`, `TypeName`, `Level`, `Datatype`, `Container`, `ObjectOps`, `CreateOption`, `OperationSpec`, and `TypeError` traits/types in `crates/types/src/datatype.rs` and `crates/types/src/lib.rs`
- [ ] T012 Implement in-memory catalog (namespace → schema → container metadata; implicit namespace on first create/write) in `crates/types/src/catalog.rs`
- [ ] T013 [P] Implement interim types `kv_collection`, `relational_table` (`columns`, `primary_key`), `document_store` (`id_field` default `_id`), `object_collection` (`versioning=false`), `vector_collection` (`dims` required, `metric ∈ cosine|l2|dot`), `ordered_map` in `crates/types/src/interim/` with accounting against buffer `types.memory` in `crates/types/src/interim/memory.rs`
- [ ] T014 Implement `QuorumLevel` (`One`…`All`, `Acks(u16)`), `OptionSource`, `ClampReason`, `Sourced<T>`, `QueryOptions`, `SessionOptions`, and `resolve()`/`clamp()` (precedence query → session → namespace → global; defaults clamp / explicit reject per FR-030) in `crates/exec/src/options.rs`
- [ ] T015 [P] Implement `LogicalRequest` variants (`Ddl`, `Point`, `Scan`, `Aggregate`, `Join`, `Mutate`, `TypeOp`, `Object`, `Batch`) and `Expr` helpers in `crates/exec/src/request.rs` per [contracts/execution-boundary.md](contracts/execution-boundary.md)
- [ ] T016 [P] Implement `QueryEngine`, `ExecutionEvent`, `ExecError`, and `PlacementInfo` (LocalEngine reports `replicas=1`) in `crates/exec/src/lib.rs`
- [ ] T017 Implement `ExecutionRecord` ring (default capacity 10_000, buffer `exec.records`) in `crates/exec/src/record.rs`
- [ ] T018 Implement single-node `LocalEngine` modules (`ddl`, `point`, `scan`, `aggregate`, `join`, `mutate`, `typeop`, `object`, `timeout`) in `crates/exec/src/local/` executing against `TypeRegistry`
- [ ] T019 Implement `Authenticator`, `Credential`, `Principal` (`namespace: Option<NamespaceName>`; `None` only for `admin` on native-selection protocols), and `AuthError` in `crates/auth/src/lib.rs`
- [ ] T020 [P] Implement `users_file` parse/validate/reload (mode ≤ `0600`, line `name secret namespace [schema=…] [roles=…]`, codes `users_file_permissions` / `users_file_syntax{line}` / `users_file_duplicate_user`) in `crates/auth/src/users_file.rs`
- [ ] T021 [P] Implement SCRAM-SHA-256 verifier helpers in `crates/auth/src/scram.rs`, SigV4 canonical-request / signing-key helpers in `crates/auth/src/sigv4.rs`, and Digest MD5 (RFC 2617/7616) in `crates/auth/src/digest.rs`
- [ ] T022 Implement `ProtocolDriver`, `ProtocolName`, `Mapping` (`Native` | `Canonical(Carrier)`), `CreateVerb`, and name escape/unescape traits in `crates/protocol-core/src/lib.rs`
- [ ] T023 [P] Implement `ClientSession` state machine (`Connecting` → `Handshaking` → `Ready`/`Executing` → `Closed` for mismatch/auth/drain) in `crates/protocol-core/src/session.rs`
- [ ] T024 [P] Implement first-bytes signature detection with `handshake_timeout` default `1s` (FR-004) in `crates/protocol-core/src/signature.rs`
- [ ] T025 [P] Implement namespace binding rules (client-selected vs credential-bound; admin override) in `crates/protocol-core/src/namespace.rs`
- [ ] T026 [P] Implement `ErrorRenderer` trait and shared helpers in `crates/protocol-core/src/errors.rs` matching [data-model.md](data-model.md) §8 error-form table
- [ ] T027 [P] Implement axum/hyper HTTP session glue (`Basic`/`Digest` extractors, `X-SpaceStorage-*` headers, `serve_connection`) in `crates/protocol-core/src/http.rs`
- [ ] T028 [P] Implement shared option literal parsing, precedence application, and inspection row builders in `crates/protocol-core/src/options.rs`
- [ ] T029 [P] Reserve protocol metric names/labels (`protocol`, `namespace`, `user`, `app`, `kind`, `error_type`) in `crates/protocol-core/src/stats.rs` for `08`
- [ ] T030 Add `query_defaults` (`write_quorum` default `TWO`, `read_quorum` default `ONE`, `timeout` default `30s` range `1ms`–`24h`, `max_timeout` default `10m` ≥ timeout), `auth { users_file }`, and `protocols { <handler> {…} }` blocks with validation codes from [contracts/config-directives.md](contracts/config-directives.md) in `crates/config/`
- [ ] T031 Register eight `ClientFacing` handler names in `HandlerRegistry` and wire `types.memory` + `exec.records` buffers in `crates/node/`; enforce FR-018 `driver_mapping_missing{handler, type}` at startup
- [ ] T032 [P] Add admin-proto types for `ExecutionRecord` / protocol map and ops `executions` + `protocols` in `crates/admin-proto/`; stub CLI commands in `crates/spacestorage/`

**Checkpoint**: Foundation ready — seams compile; config parses `query_defaults`/`auth`/`protocols`; no handler smoke yet

---

## Phase 3: User Story 1 - Connect with an existing client through its own protocol port (Priority: P1) 🎯 MVP

**Goal**: Eight handlers on distinct entrypoints accept stock clients, complete native handshake/auth, bind exactly one namespace per session, and complete create/write/read/delete smoke. Wrong-port mismatch refuses within 1s. Credential-bound protocols refuse credentials without a namespace.

**Independent Test**: Declare one entrypoint per handler; stock client smoke per handler; connect each client to a foreign port and confirm refusal within 1s (SC-001, SC-008, scenarios 12–13).

### Tests for User Story 1 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T033 [P] [US1] Add conformance harness boot (8 handlers on port `0`, users file, client factories) in `crates/conformance/tests/harness/mod.rs`
- [ ] T034 [P] [US1] Add smoke tests `crates/conformance/tests/smoke_redis.rs` and `crates/conformance/tests/smoke_postgresql.rs` (SC-001 create/write/read/delete)
- [ ] T035 [P] [US1] Add smoke tests `crates/conformance/tests/smoke_{elasticsearch,s3,webdav,cassandra,clickhouse,clickhouse_http}.rs`
- [ ] T036 [P] [US1] Add mismatch matrix `crates/conformance/tests/mismatch_matrix.rs` (cross-protocol connect refused ≤1s, SC-008)
- [ ] T037 [P] [US1] Add namespace isolation tests (scenarios 12–13) in `crates/conformance/tests/namespace_isolation.rs`
- [ ] T038 [P] [US1] Add drain behaviour test in `crates/conformance/tests/drain.rs` (FR-040)
- [ ] T039 [P] [US1] Add fixture contract tests in `crates/conformance/tests/fixtures.rs` validating every file under `specs/002-protocol-drivers/contracts/fixtures/` (SC-010)

### Implementation for User Story 1

- [ ] T040 [US1] Implement Redis handler handshake, AUTH (credential-bound namespace), PING/SET/GET/DEL smoke path in `crates/handler-redis/src/{handler.rs,auth.rs,commands/,lower.rs,render.rs}` registering handler name `redis`
- [ ] T041 [US1] Wire Redis handler into node registry and entrypoint serve path in `crates/node/` so `entrypoint.handler redis` listens
- [ ] T042 [US1] Implement PostgreSQL handler via `pgwire` (SSLRequest/TLS, SCRAM-SHA-256, database name = namespace) with simple CREATE/INSERT/SELECT/DROP smoke in `crates/handler-postgresql/src/{handler.rs,auth.rs,lower/,render.rs,catalog_views.rs}`
- [ ] T043 [US1] Implement Elasticsearch HTTP handler (Basic auth, `GET /`, index create, doc index, search smoke) in `crates/handler-elasticsearch/src/{router.rs,auth.rs,api/,dsl/,render.rs}`
- [ ] T044 [P] [US1] Implement S3 handler (SigV4, CreateBucket/PutObject/GetObject/List/Delete including multipart) in `crates/handler-s3/src/{router.rs,sigv4_extract.rs,xml.rs,ops/,render.rs}`
- [ ] T045 [P] [US1] Implement WebDAV handler (Basic/Digest, PROPFIND/MKCOL/PUT/GET/MOVE/DELETE smoke) in `crates/handler-webdav/src/{router.rs,auth.rs,methods/,props.rs,locks.rs,xml.rs,render.rs}`
- [ ] T046 [US1] Implement Cassandra CQL v4/v5 frames, SASL PLAIN auth, keyspace=namespace, CREATE/INSERT/SELECT/DROP smoke in `crates/handler-cassandra/src/{handler.rs,auth.rs,cql/,lower.rs,render.rs,system_tables.rs}`
- [ ] T047 [US1] Implement ClickHouse native (`clickhouse`) Hello/Query/Data path with CityHash 1.0.2 in `crates/handler-clickhouse/src/native/` and HTTP (`clickhouse-http`) router/formats in `crates/handler-clickhouse/src/http/`; register both handler names from `crates/handler-clickhouse/src/lib.rs`
- [ ] T048 [US1] Apply signature detection + protocol mismatch refusal (FR-004/FR-005) and TLS entrypoint behaviour (FR-008) across all handlers using `crates/protocol-core/src/signature.rs`
- [ ] T049 [US1] Enforce credential-bound namespace refusal when `Principal.namespace` is `None` on redis/s3/webdav/elasticsearch in `crates/handler-redis/src/auth.rs`, `crates/handler-s3/src/sigv4_extract.rs`, `crates/handler-webdav/src/auth.rs`, and `crates/handler-elasticsearch/src/auth.rs` (FR-009a)
- [ ] T050 [US1] Honour node drain for protocol sessions (refuse new, finish in-flight, close idle) in `crates/protocol-core/src/session.rs` and handler serve loops

**Checkpoint**: All eight smoke tests pass; mismatch and namespace-isolation tests pass; MVP demoable with stock clients

---

## Phase 4: User Story 2 - See and use the whole type system through any protocol (Priority: P1)

**Goal**: Every interim type is listable, creatable (type option on native create verb; default type when omitted), readable/writable (native or single canonical fallback), alterable, droppable, and operable from every protocol without hiding types.

**Independent Test**: Create one container of every interim type through each protocol; list/read/write/alter/drop through every other protocol; compare logical content (SC-002, SC-003).

### Tests for User Story 2 ⚠️

- [ ] T051 [P] [US2] Add type×protocol matrix and canonical write-back tests in `crates/conformance/tests/type_matrix.rs` (SC-002, SC-003)
- [ ] T052 [P] [US2] Add canonical vector unit tests under `crates/types/tests/canonical_vectors/` and `crates/types/tests/canonical.rs`
- [ ] T053 [P] [US2] Add interim type unit tests in `crates/types/tests/interim_types.rs` (create options, knn/range ops, memory accounting)

### Implementation for User Story 2

- [ ] T054 [P] [US2] Declare per-protocol `mapping()` for all six interim types (native or `Canonical(Carrier)`) in each `crates/handler-*/src/lib.rs` / driver module per [contracts/protocols/](contracts/protocols/) and [contracts/canonical-representation.md](contracts/canonical-representation.md)
- [ ] T055 [US2] Implement type option on native create verbs and documented defaults (`relational_table` for SQL/CQL/CH, `kv_collection` Redis, `document_store` ES, `object_collection` S3/WebDAV) in `crates/handler-postgresql/src/lower/ddl.rs`, `crates/handler-cassandra/src/lower.rs`, `crates/handler-clickhouse/src/sql/lower.rs`, `crates/handler-redis/src/commands/ss.rs`, `crates/handler-elasticsearch/src/api/index.rs`, `crates/handler-s3/src/ops/bucket.rs`, and `crates/handler-webdav/src/methods/mkcol.rs` (FR-014a)
- [ ] T056 [US2] Implement namespace listing/describe that always includes SpaceStorage type name for every container in `crates/handler-postgresql/src/catalog_views.rs`, `crates/handler-cassandra/src/system_tables.rs`, `crates/handler-redis/src/commands/keys.rs`, `crates/handler-elasticsearch/src/api/cat.rs`, `crates/handler-clickhouse/src/system_tables.rs`, `crates/handler-s3/src/ops/list.rs`, and `crates/handler-webdav/src/methods/propfind.rs` (FR-013)
- [ ] T057 [US2] Implement canonical carrier read/write (unaltered bytes) and native representation paths in `crates/handler-*/src/render.rs` and corresponding lower modules under `crates/handler-{postgresql,cassandra,redis,elasticsearch,clickhouse,s3,webdav}/src/` (FR-015, FR-015a)
- [ ] T058 [US2] Implement type-specific op surfaces (`SS.*`, `_spacestorage`, `spacestorage.*`, `?spacestorage-op`, WebDAV `REPORT`) including `knn` and `range` in `crates/handler-redis/src/commands/ss.rs`, `crates/handler-elasticsearch/src/api/spacestorage.rs`, `crates/handler-postgresql/src/lower/`, `crates/handler-s3/src/ops/spacestorage.rs`, and `crates/handler-webdav/src/methods/report.rs`
- [ ] T059 [US2] Implement alter/drop mappings so one shared container is visible across protocols in `crates/handler-postgresql/src/lower/ddl.rs`, `crates/handler-cassandra/src/lower.rs`, `crates/handler-redis/src/commands/ss.rs`, `crates/handler-elasticsearch/src/api/index.rs`, `crates/handler-clickhouse/src/sql/lower.rs`, `crates/handler-s3/src/ops/bucket.rs`, and `crates/handler-webdav/src/methods/get_put_delete.rs` (Story 2 scenarios 10–11)
- [ ] T060 [US2] Reject type-mismatched writes with protocol error forms naming container type and field in each `crates/handler-*/src/render.rs` (FR-016); reversible name escaping in `crates/protocol-core/src/escape.rs` used by those handlers (FR-019)

**Checkpoint**: `type_matrix` conformance green; startup fails only if a declared driver omits a mapping

---

## Phase 5: User Story 3 - Control quorum and timeout on every query (Priority: P2)

**Goal**: Every executed request carries `QueryOptions` (Cassandra vocabulary + timeout) with precedence and clamping; clients inspect applied values; timeouts and unsatisfiable explicit quorums use native error forms.

**Independent Test**: On a single-replica node, record options via admin `executions`; verify defaults (`TWO`/`ONE`/`30s`), clamping of default `TWO`→`ONE`, rejection of explicit `TWO`, session/per-query overrides (SC-004, SC-005).

### Tests for User Story 3 ⚠️

- [ ] T061 [P] [US3] Add options conformance in `crates/conformance/tests/options.rs` (defaults, clamp, explicit reject, precedence, inspection; SC-004)
- [ ] T062 [P] [US3] Add timeout deadline test in `crates/conformance/tests/timeout_deadline.rs` (error within deadline + 1s; SC-005)
- [ ] T063 [P] [US3] Add unit tests for `resolve`/`clamp` in `crates/exec/tests/options.rs`

### Implementation for User Story 3

- [ ] T064 [US3] Wire Cassandra native consistency/timeout fields into `QueryOptions` in `crates/handler-cassandra/src/lower.rs`
- [ ] T065 [P] [US3] Implement per-session and per-query option syntax for PostgreSQL/ClickHouse (settings), Redis (commands), and HTTP headers/params for ES/S3/WebDAV/CH-HTTP in `crates/handler-postgresql/src/lower/options.rs`, `crates/handler-clickhouse/src/http/session.rs`, `crates/handler-redis/src/commands/server.rs`, `crates/handler-elasticsearch/src/api/`, `crates/handler-s3/src/router.rs`, and `crates/handler-webdav/src/router.rs` per [contracts/query-options.md](contracts/query-options.md)
- [ ] T066 [US3] Apply `protocol-core` option resolution on every `LogicalRequest` submit path so absent options use global write `TWO` / read `ONE` / timeout `30s` via `crates/protocol-core/src/options.rs` called from each handler execute glue
- [ ] T067 [US3] Implement session inspection mechanisms per protocol returning applied quorum/timeout and source (including `"global default, clamped to replica count"`) in `crates/handler-postgresql/src/lower/options.rs`, `crates/handler-cassandra/src/system_tables.rs`, `crates/handler-redis/src/commands/server.rs`, and HTTP `X-SpaceStorage-*` responses in `crates/protocol-core/src/http.rs` (FR-025)
- [ ] T068 [US3] Enforce timeout cancellation in `crates/exec/src/local/timeout.rs` and render protocol timeout errors with applied value (FR-029); clamp client timeouts to `max_timeout` (FR-028)
- [ ] T069 [US3] Reject invalid option values without mutating session options (FR-031) in `crates/protocol-core/src/options.rs` and handlers

**Checkpoint**: Options + timeout conformance pass; every `ExecutionRecord` shows sourced quorum/timeout

---

## Phase 6: User Story 4 - Configure protocol ports and global query defaults (Priority: P2)

**Goal**: Operators declare any subset of handlers, set live-reloadable `query_defaults`, and inspect protocol map + defaults via effective config, admin API, and CLI. Invalid quorum/timeout fail startup.

**Independent Test**: Start with full set, subset, bad defaults, missing users file; read effective config and `spacestorage protocols` (SC-009, SC-010).

### Tests for User Story 4 ⚠️

- [ ] T070 [P] [US4] Extend `crates/conformance/tests/fixtures.rs` assertions for `invalid/{bad-quorum,zero-timeout,missing-users-file,two-handlers-one-port,users-no-namespace}.conf` codes from [contracts/config-directives.md](contracts/config-directives.md)
- [ ] T071 [P] [US4] Add admin/CLI protocol-map assertions in `crates/conformance/tests/fixtures.rs` or `crates/spacestorage` tests: subset config opens only declared listeners; omitted defaults report built-in provenance

### Implementation for User Story 4

- [ ] T072 [US4] Ensure subset of declared handlers is the only set that binds sockets; effective config lists declared vs not-declared in `crates/node/` + `crates/config/` resolve (FR-007)
- [ ] T073 [US4] Implement live reload of `query_defaults` (in-flight keep old values; new queries use new defaults) in `crates/config/` reload path and `crates/node/` (FR-027)
- [ ] T074 [US4] Complete admin `GET /v1/protocols` and `GET /v1/executions` plus CLI `spacestorage protocols` / `spacestorage executions` in `crates/node/`, `crates/admin-proto/`, `crates/spacestorage/` (FR-041)
- [ ] T075 [US4] Report handler version ranges, connection counts, transport flags, and global defaults provenance (`configured | built_in`) in effective config serialization in `crates/config/` / `crates/node/`
- [ ] T076 [US4] Require `auth.users_file` when any client-facing handler is declared (`auth_users_file_required`) in `crates/config/` validation

**Checkpoint**: Invalid fixtures fail with named codes; CLI shows protocol map; subset node has no undeclared listeners

---

## Phase 7: User Story 5 - Reach the same data from any node through any protocol (Priority: P3)

**Goal**: Every node that declares a handler is a coordinator; `PlacementInfo` seam allows future remote routing without driver changes. Cross-node read-after-write is specified but deferred until `04`/`06`.

**Independent Test**: Two-node cluster write A/protocol X → read B/protocol Y (SC-007). Until membership exists, test is `#[ignore]`d with reason.

### Tests for User Story 5 ⚠️

- [ ] T077 [P] [US5] Add `#[ignore = "needs 04/06 cluster membership"]` test `crates/conformance/tests/cross_node_read_after_write.rs` (SC-007) documenting the two-node workflow
- [ ] T078 [P] [US5] Add unit test that `LocalEngine` `PlacementInfo` returns `replicas=1` and `satisfiable` behaviour for `TWO` in `crates/exec/tests/local_engine.rs`

### Implementation for User Story 5

- [ ] T079 [US5] Keep all handlers available on every node that declares them; no protocol-only or data-only mode in `crates/node/` registration (FR-032, FR-033)
- [ ] T080 [US5] Expose per-node protocol map in cluster effective-config aggregation surface (or documented single-node placeholder) in `crates/node/` / admin status so asymmetry is visible (FR-034)
- [ ] T081 [US5] Ensure drivers submit only through `QueryEngine`/`PlacementInfo` traits (no storage crate deps in handler `Cargo.toml`) enforcing FR-035 compile-time boundary

**Checkpoint**: Ignored SC-007 test compiles; handlers have no direct engine-internals dependencies

---

## Phase 8: User Story 6 - Native queries in every protocol run on the shared execution layer (Priority: P3)

**Goal**: Protocol languages/verbs lower to `LogicalRequest` and execute only via shared `QueryEngine`; unsupported features return native not-supported errors; execution records carry protocol name; equivalent queries agree.

**Independent Test**: Load one dataset; run equivalent filter/aggregate/range/knn queries across protocols; compare results and `ExecutionRecord` protocol attachment (SC-006).

### Tests for User Story 6 ⚠️

- [ ] T082 [P] [US6] Add cross-protocol equivalence suite in `crates/conformance/tests/equivalence.rs` (SC-006)
- [ ] T083 [P] [US6] Add `ExecutionRecord` / ring unit tests in `crates/exec/tests/records.rs`
- [ ] T084 [P] [US6] Add LocalEngine not-supported / multi-type join behaviour tests in `crates/exec/tests/local_engine.rs`

### Implementation for User Story 6

- [ ] T085 [US6] Complete PostgreSQL SQL lowering (DDL/DML/SELECT/catalog) to `LogicalRequest` in `crates/handler-postgresql/src/lower/`
- [ ] T086 [US6] Complete ClickHouse SQL lowering + functions shared by native and HTTP in `crates/handler-clickhouse/src/sql/`
- [ ] T087 [US6] Complete CQL parser→`LogicalRequest` including `USING`/consistency already mapped in `crates/handler-cassandra/src/{cql/,lower.rs}`
- [ ] T088 [P] [US6] Complete Redis command→`LogicalRequest` (strings/hashes/keys/`SS.*`) in `crates/handler-redis/src/{commands/,lower.rs}`
- [ ] T089 [P] [US6] Complete Elasticsearch DSL/REST→`LogicalRequest` in `crates/handler-elasticsearch/src/dsl/`
- [ ] T090 [P] [US6] Complete S3 and WebDAV object/collection ops→`Object`/`Mutate`/`Ddl` requests in `crates/handler-s3/src/ops/` and `crates/handler-webdav/src/methods/`
- [ ] T091 [US6] Ensure every execute path records protocol name on `ExecutionRecord` in `crates/exec/src/record.rs` + engine submit glue (FR-037)
- [ ] T092 [US6] Map unsupported protocol features and verbs outside MUST sets to native not-supported errors without partial execution (FR-038) in each `crates/handler-*/src/render.rs`

**Checkpoint**: Equivalence suite green; admin `executions` shows protocol-tagged records for language and non-language ops

---

## Phase 9: Polish & Cross-Cutting Concerns

**Purpose**: Documentation, observability hooks, version pins, quickstart proof

- [ ] T093 [P] Generate operator docs `docs/protocols/{postgresql,cassandra,redis,elasticsearch,clickhouse,s3,webdav}.md` from contracts (type-mapping tables, option syntax, version ranges) per FR-043
- [ ] T094 [P] Emit FR-042 counters/histograms into `crates/node` stats (labelled `protocol`/`namespace`; exposition remains `08`)
- [ ] T095 Pin announced protocol versions (PG `16.4`, ES `8.15.0`, CH `54460`, Redis `7.2.0`, CQL v4/v5) and conformance client crate versions; document bump as explicit change in handler `Cargo.toml` / `crates/conformance/Cargo.toml`
- [ ] T096 Run and fix `specs/002-protocol-drivers/quickstart.md` walkthrough against a local build (SC-001–SC-006, SC-008–SC-011); note SC-007 deferred
- [ ] T097 [P] Add unit tests for auth vectors in `crates/auth/tests/{users_file.rs,sigv4_vectors.rs,digest.rs}`
- [ ] T098 Code cleanup: ensure handler crates depend only on `protocol-core`, `types`, `exec`, `auth` (no storage/engine internals) via `Cargo.toml` review
- [ ] T099 Document in-memory durability limitation until `03` in `docs/examples/node-all-protocols.conf` comments and protocol docs

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Setup — **BLOCKS** all user stories
- **User Stories (Phases 3–8)**: All depend on Foundational
  - Prefer priority order: US1 → US2 → US3 → US4 → US5 → US6
  - After Foundational, US4 config surfaces can proceed in parallel with late US1 handlers if staffed carefully (shared `crates/config` / `crates/node` files)
- **Polish (Phase 9)**: Depends on desired stories being complete (typically after US1–US4 minimum; US5–US6 for full SC set)

### User Story Dependencies

- **US1 (P1)**: After Foundational — no dependency on other stories — **MVP**
- **US2 (P1)**: After Foundational; practically needs US1 smoke paths to attach mappings
- **US3 (P2)**: Needs US1 execute path to attach options; benefits from US2 type ops
- **US4 (P2)**: Config parsing is Foundational; this story completes operator/CLI/reload surfaces (can overlap late US1)
- **US5 (P3)**: Mostly seam discipline + ignored multi-node test; independent of US6
- **US6 (P3)**: Needs US1–US3 lowering/options; deepens shared-engine contract

### Within Each User Story

- Tests (where listed) MUST be written and FAIL before implementation
- Models/seams before handlers
- Handler smoke before type matrix / options / equivalence
- Story complete before moving to next priority when staffing is serial

### Parallel Opportunities

- Phase 1 crate skeletons marked [P]
- Phase 2: types canonical/ident, auth modules, protocol-core modules, exec request/options in parallel after traits exist
- US1: after Redis path proves end-to-end, HTTP handlers (ES/S3/WebDAV) in parallel; Cassandra and ClickHouse after
- US2 mapping declarations per handler [P]
- US3 per-protocol option syntax modules [P]
- US6 Redis/ES/S3/WebDAV lowering [P] after SQL/CQL cores land
- Conformance tests marked [P] within a story

---

## Parallel Example: User Story 1

```bash
# After T033 harness exists, launch smoke test stubs together:
Task: "Add smoke tests crates/conformance/tests/smoke_redis.rs and smoke_postgresql.rs"
Task: "Add smoke tests crates/conformance/tests/smoke_{elasticsearch,s3,webdav,cassandra,clickhouse,clickhouse_http}.rs"
Task: "Add mismatch matrix crates/conformance/tests/mismatch_matrix.rs"

# After Redis path works, HTTP handlers in parallel:
Task: "Implement Elasticsearch HTTP handler in crates/handler-elasticsearch/src/..."
Task: "Implement S3 handler in crates/handler-s3/src/..."
Task: "Implement WebDAV handler in crates/handler-webdav/src/..."
```

---

## Parallel Example: User Story 2

```bash
Task: "Add type_matrix.rs conformance tests"
Task: "Add canonical vector unit tests under crates/types/tests/"
Task: "Declare per-protocol mapping() for all six interim types in each handler crate"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL)
3. Complete Phase 3: User Story 1 — recommend implementing **redis then postgresql** first to prove the full path, then remaining six handlers
4. **STOP and VALIDATE**: stock-client smokes + mismatch + namespace isolation
5. Demo/deploy single-node multi-protocol node

### Incremental Delivery

1. Setup + Foundational → seams ready
2. US1 → stock clients connect (MVP)
3. US2 → full type visibility/lifecycle
4. US3 → quorum/timeout everywhere
5. US4 → operator config/CLI polish
6. US5 → coordinator seam + ignored SC-007
7. US6 → deep native lowering + equivalence
8. Polish → docs/quickstart/metrics

### Parallel Team Strategy

1. Team completes Setup + Foundational together
2. Then:
   - Dev A: Redis + PostgreSQL (US1 core)
   - Dev B: ES + S3 + WebDAV (US1 HTTP)
   - Dev C: Cassandra + ClickHouse (US1 remaining) then US6 lowering depth
3. Shared follow-ons: US2 mappings, US3 options, US4 admin/CLI

---

## Notes

- [P] = different files, no dependencies on incomplete tasks
- [Story] label maps task to US1–US6 for traceability
- Tests included because plan/spec explicitly require the conformance suite and SC criteria
- First shippable **binary** subset (PostgreSQL+Redis only) is owned by feature `016`; this feature still implements the complete eight-handler product surface
- SC-007 remains `#[ignore]` until `04`/`06`
- Commit after each task or logical group; stop at checkpoints to validate independently
- Avoid: protocol-private engines, per-protocol type systems, secrets inlined in config, altering canonical bytes in drivers
