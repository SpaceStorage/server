---
description: "Task list for SpaceStorage Runtime, CLI, and Node Interfaces"
---

# Tasks: SpaceStorage Runtime, CLI, and Node Interfaces

**Input**: Design documents from `/Users/g.kashintsev/devel/myrepo/spacestorage/server/specs/001-runtime-cli-api/`

**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md, data-model.md, contracts/

**Tests**: Included — plan.md Technical Context explicitly requires `cargo test` unit tests per crate, integration tests in `crates/node/tests/`, contract tests against `contracts/fixtures/`, and CLI tests via `assert_cmd`-style process spawning.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3, US4)
- Include exact file paths in descriptions

## Path Conventions

- Workspace root: repository root (`Cargo.toml`, `rust-toolchain.toml`)
- Libraries: `crates/config/`, `crates/admin-proto/`, `crates/node/`
- Binaries: `crates/spacestoraged/`, `crates/spacestorage/`
- Docs: `docs/`
- Spec fixtures: `specs/001-runtime-cli-api/contracts/fixtures/`

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Cargo workspace, toolchain pin, and empty crate layout matching plan.md

- [ ] T001 Create workspace root `Cargo.toml` with members `crates/config`, `crates/admin-proto`, `crates/node`, `crates/spacestoraged`, `crates/spacestorage` and shared `[workspace.dependencies]`
- [ ] T002 [P] Add toolchain pin file `./rust-toolchain.toml` (Rust 1.87, edition 2024, MSRV 1.85)
- [ ] T003 [P] Scaffold `crates/config/Cargo.toml` + `crates/config/src/lib.rs` (no Tokio dependency)
- [ ] T004 [P] Scaffold `crates/admin-proto/Cargo.toml` + `crates/admin-proto/src/lib.rs` (serde/bytes only)
- [ ] T005 [P] Scaffold `crates/node/Cargo.toml` + `crates/node/src/lib.rs` module tree placeholders (`runtime`, `lifecycle`, `entrypoint`, `handler`, `admin`, `buffer`, `reload`, `stats`, `effective`)
- [ ] T006 [P] Scaffold `crates/spacestoraged/Cargo.toml` + `crates/spacestoraged/src/main.rs` stub
- [ ] T007 [P] Scaffold `crates/spacestorage/Cargo.toml` + `crates/spacestorage/src/{main,args,output,exit}.rs` and `crates/spacestorage/src/client/` stubs
- [ ] T008 Wire workspace dependency versions for `tokio`, `axum`, `hyper`, `rustls`, `tokio-rustls`, `rustls-pemfile`, `webpki-roots`, `serde`, `serde_json`, `clap`, `tracing`, `tracing-subscriber`, `bytes`, `arc-swap`, `tokio-util`, `assert_cmd`, `predicates` per plan.md / research.md

**Checkpoint**: `cargo check --workspace` succeeds with empty stubs

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Shared config parser, admin-proto types, handler/buffer registries, auth seam, effective-config snapshot — MUST complete before any user story

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

### Config crate (offline + online shared validation)

- [ ] T009 Implement `ConfigError { file, line, col, setting, code, message }` and error codes in `crates/config/src/error.rs`
- [ ] T010 [P] Implement nginx-style lexer (IDENT, NUMBER, SIZE, DURATION, STRING, PATH, `{` `}` `;`, `#` comments) in `crates/config/src/lexer.rs`
- [ ] T011 [P] Implement AST (`Directive` | `Block`) in `crates/config/src/ast.rs`
- [ ] T012 Implement recursive-descent parser collecting all syntax errors in `crates/config/src/parser.rs` per `contracts/config-grammar.md`
- [ ] T013 Implement typed model `NodeConfig`, `EntrypointDecl`, `TlsDecl`, `AdminHandlerDecl`, `CertRef` in `crates/config/src/model.rs` with constraints: `node.name` non-empty ≤ 253 chars; `runtime.threads` if set ≥ 1; `runtime.drain_timeout` 1 s–24 h (default 30 s); `log.level` one of `error|warn|info|debug|trace` (default `info`); `log.format` `text|json` (default `text`); entrypoint `port` 1–65535 mandatory; default `address` `127.0.0.1`
- [ ] T014 Implement `resolve()` AST → `NodeConfig` applying defaults, `threads auto`, launch `--set` overrides, and accumulating ALL resolve errors in `crates/config/src/resolve.rs`
- [ ] T015 Implement `validate()` cross-field rules in `crates/config/src/validate.rs`: `admin_handler_undeclared` / `admin_handler_conflict`; `admin_token_required` / `admin_token_unreadable` (token file readable, non-empty after trim when any admin handler enabled); entrypoint missing/multiple/unknown handler; duplicate `(address,port)` / name; `threads_out_of_range`; `drain_timeout_out_of_range`; `buffer_unknown` / `buffer_out_of_range`; `cert_inline_forbidden` / `cert_ref_scheme_unsupported` / cert unreadable/invalid/expired; every enabled entrypoint MUST declare `tls { ... }` or `plaintext;` (omitted transport → startup error); reserved foreign blocks (`labels`, `storage`, …) → `unknown_directive`
- [ ] T016 [P] Implement `ReloadClass { Live, RestartRequired }` and `diff(running, incoming) -> ConfigDiff` in `crates/config/src/reload_class.rs` (buffers, drain_timeout, log.level, token_file = Live; threads, entrypoints, log.format = RestartRequired)
- [ ] T017 Export public `parse_validate(path, overrides, handler_names, buffer_specs) -> Result<(NodeConfig, Vec<warning>), Vec<ConfigError>>` from `crates/config/src/lib.rs`

### Admin-proto + node skeletons

- [ ] T018 [P] Implement shared JSON types `Status`, `EffectiveConfig`, `BufferReport`, `ReloadReport`, `ErrorBody`, `ThreadsReport` in `crates/admin-proto/src/types.rs` per `contracts/admin-api.md` and data-model.md
- [ ] T019 [P] Implement `AdminOp { Status, Config, Threads, Buffers, Reload, Stop { wait } }` in `crates/admin-proto/src/ops.rs`
- [ ] T020 [P] Implement length-prefixed JSON frame codec (`u32` BE len, payload 1..=4 MiB) plus Hello/Request/Response shapes in `crates/admin-proto/src/frame.rs` per `contracts/admin-tcp-protocol.md`
- [ ] T021 Implement `Handler` trait (`name`, `kind`, `owner`, `serve`) and `HandlerRegistry` with duplicate-name rejection in `crates/node/src/handler/mod.rs`
- [ ] T022 [P] Implement `Buffer` / `BufferRegistry` / `OverflowPolicy { Reject, Wait }` / `Permit` and register builtins in `crates/node/src/buffer/{mod,builtin}.rs`: `net.recv` default 64 MiB range 1 MiB–64 GiB Wait; `net.send` default 64 MiB range 1 MiB–64 GiB Wait; `request.queue` default 16 MiB range 1 MiB–16 GiB Reject — including `try_reserve`/`set_capacity` (allow capacity &lt; used; ratio may exceed 1.0) per FR-038–FR-042
- [ ] T023 [P] Implement in-memory `Stats` atomics (`worker_threads`, `worker_threads_busy`, `drain_timed_out`, uptime derivation) with Prometheus-conformant reserved names in `crates/node/src/stats.rs`
- [ ] T024 Implement `EffectiveConfig` snapshot via `ArcSwap` and pending-restart markers in `crates/node/src/effective.rs`
- [ ] T025 Implement interim bearer `token_file` constant-time check in `crates/node/src/admin/auth.rs` (health probes exempt)
- [ ] T026 Assemble `Node` shell fields (`state`, `effective`, `handlers`, `buffers`, `stats`, `tasks`, `cancel`, `reload_lock`) in `crates/node/src/lib.rs`
- [ ] T027 [P] Add contract tests that parse every fixture under `specs/001-runtime-cli-api/contracts/fixtures/` (including `invalid/*.conf`) and assert expected validation codes in `crates/config/tests/fixtures.rs` (use test-only stub `cassandra` handler inventory so `fixtures/node.conf` passes SC-011)

**Checkpoint**: Foundation ready — config validates fixtures offline; registries and types compile; user story implementation can begin

---

## Phase 3: User Story 1 - Start a node sized to the machine (Priority: P1) 🎯 MVP

**Goal**: Single Tokio multi-thread process sized to available cores (or override), reaches `ready`, serves concurrent work without head-of-line blocking, graceful drain/stop within drain timeout

**Independent Test**: Start one node from a config on machines with different core counts; observe reported worker thread count and `ready`; issue concurrent short requests beside a long one; stop and confirm drain/exit behavior (SC-001, SC-002, SC-003, SC-010)

### Tests for User Story 1

- [ ] T028 [P] [US1] Write integration tests for startup thread derivation, invalid threads rejection before bind, and ready timing in `crates/node/tests/startup.rs` (SC-001, SC-002, SC-004 threads case)
- [ ] T029 [P] [US1] Write drain/stop integration tests (in-flight complete within timeout; timeout records `drain_timed_out`; second signal immediate exit; stop before ready aborts) in `crates/node/tests/drain.rs` (SC-010, FR-006/007)

### Implementation for User Story 1

- [ ] T030 [US1] Implement Tokio multi-thread runtime builder (`worker_threads` = configured or `available_parallelism()` min 1; fallback to 1 + log when undetermined) in `crates/node/src/runtime.rs`
- [ ] T031 [US1] Implement `NodeState` machine `Starting|Ready|Draining|Failed`, SIGTERM/SIGINT handling, and drain orchestration with `TaskTracker` + `CancellationToken` in `crates/node/src/lifecycle.rs` (`drain_timeout` default 30 s, range 1 s–24 h)
- [ ] T032 [US1] Implement listener bind-all-before-ready, accept loop → `handler.serve`, and unbind-on-abort in `crates/node/src/entrypoint/{mod,listener}.rs` (default address `127.0.0.1` when omitted)
- [ ] T033 [US1] Add a test-only / stub client-facing handler usable for concurrency and drain tests in `crates/node/src/handler/` (or test helper) so US1 can exercise FR-004 without admin handlers
- [ ] T034 [US1] Implement `spacestoraged` binary: parse `--config` and `--set key=value`, build runtime, load config, run node in `crates/spacestoraged/src/main.rs`
- [ ] T035 [US1] Sample busy worker count into `Stats` (RuntimeMetrics or in-flight task fallback per research R3) from `crates/node/src/stats.rs` / lifecycle hooks
- [ ] T036 [US1] Ensure stop-before-ready closes any opened listeners and exits without entering `ready` in `crates/node/src/lifecycle.rs` + `crates/node/src/entrypoint/mod.rs`

**Checkpoint**: Node starts, sizes threads, reaches `ready`, drains/stops cleanly — MVP runtime without requiring admin CLI

---

## Phase 4: User Story 2 - Declare entrypoints and enable admin handlers (Priority: P1)

**Goal**: Entrypoint model (exactly one handler per address:port); mandatory explicit enable/disable for `admin` and `admin-http`; TLS or plaintext declared; both admin transports expose identical ops with interim token auth

**Independent Test**: Four admin enable/disable combinations; omit declaration fails; unknown handler fails; TLS vs plaintext; status over both transports (SC-004, SC-005, SC-006, FR-020/021/027)

### Tests for User Story 2

- [ ] T037 [P] [US2] Write admin parity tests (same JSON results over `admin` and `admin-http`, ignoring `uptime_seconds` / `connections_active` / `threads.busy`) in `crates/node/tests/admin_parity.rs`
- [ ] T038 [P] [US2] Write TLS/plaintext entrypoint tests (refuse plaintext on TLS port; invalid cert refs fail startup; omitted transport fails; `plaintext;` accepted) in `crates/node/tests/tls.rs` (SC-006)

### Implementation for User Story 2

- [ ] T039 [P] [US2] Implement rustls acceptor from file-referenced PEM cert/key with leaf validity check at startup in `crates/node/src/entrypoint/tls.rs` (no plaintext fallback; hourly expiry marker on running entrypoints)
- [ ] T040 [US2] Register built-in `admin` and `admin-http` handlers in `HandlerRegistry` from `crates/node/src/handler/mod.rs` (inventory reported in effective config)
- [ ] T041 [US2] Implement `AdminService` executing `Status`, `Config`, `Threads`, `Buffers`, `Stop` against `Node` in `crates/node/src/admin/mod.rs` per `contracts/admin-api.md`
- [ ] T042 [US2] Implement `admin` TCP handler (5 s hello timeout, bearer in hello, framed ops, idle 10 m, reserve `net.recv`/`net.send`) in `crates/node/src/handler/admin_tcp.rs` per `contracts/admin-tcp-protocol.md`
- [ ] T043 [US2] Implement `admin-http` axum router (`GET /v1/status|config|threads|buffers`, `POST /v1/reload|stop`, `GET /v1/health/live|ready`, `/metrics` → 404, headers `X-SpaceStorage-Node` / `X-SpaceStorage-State`, body limit 1 MiB) in `crates/node/src/handler/admin_http.rs` per `contracts/admin-http.md`
- [ ] T044 [US2] Enforce admin handler declarations at startup: missing → fail; `disable admin;` / `disable admin-http;` logs notice and opens no listener; duplicate address:port / port-in-use → fail naming entrypoints; bind failure rolls back all listeners in `crates/node/src/entrypoint/mod.rs` + `crates/config/src/validate.rs`
- [ ] T045 [US2] Flag each entrypoint `transport: encrypted|plaintext` and `cert_expired` in effective config / status in `crates/node/src/effective.rs`
- [ ] T046 [US2] Reject non-handler traffic on admin ports (FR-019) inside `crates/node/src/handler/admin_tcp.rs` and `crates/node/src/handler/admin_http.rs` (404/`unknown_op` for foreign paths; TCP non-ops rejected)
- [ ] T047 [US2] Implement reload orchestration parse→validate→diff→apply live (buffer capacities, drain_timeout, log.level, re-read token) + pending-restart markers; reject when state ≠ `ready`; serialize with `reload_lock` in `crates/node/src/reload.rs` and wire `AdminOp::Reload` in `crates/node/src/admin/mod.rs` (SC-009 live path; restart-required reporting for threads/entrypoints)

**Checkpoint**: Remotely administrable node over TCP and/or HTTP with TLS optional-but-explicit

---

## Phase 5: User Story 3 - Operate a node locally with the bundled CLI (Priority: P2)

**Goal**: Bundled `spacestorage` CLI validates configs offline (same `crates/config` as the node), inspects/reloads/stops via `admin` or `admin-http`, human and JSON output, distinct exit codes

**Independent Test**: `validate` on valid/invalid fixtures without a node; then status/config/threads/buffers/reload/stop against a running node (SC-007, FR-031–FR-037)

### Tests for User Story 3

- [ ] T048 [P] [US3] Write CLI process tests (`validate` multi-error exit 2; valid prints effective threads; status/stop/reload exit codes 0/3/5; JSON field stability) in `crates/spacestorage/tests/cli.rs` using `assert_cmd`

### Implementation for User Story 3

- [ ] T049 [US3] Implement clap derive command tree and global flags (`--endpoint`, `--via`, `--token-file`, `--tls`, `--ca`, `--timeout` default 5 s, `--output human|json`) in `crates/spacestorage/src/args.rs` per `contracts/cli.md` (no `--insecure`)
- [ ] T050 [P] [US3] Implement exit code mapping (0/1/2/3/4/5) in `crates/spacestorage/src/exit.rs`
- [ ] T051 [P] [US3] Implement human table + JSON output helpers in `crates/spacestorage/src/output.rs`
- [ ] T052 [US3] Implement `AdminClient` trait plus TCP framed client in `crates/spacestorage/src/client/{mod,tcp}.rs`
- [ ] T053 [US3] Implement HTTP client for admin-http routes in `crates/spacestorage/src/client/http.rs`
- [ ] T054 [US3] Implement CLI rustls client with `webpki-roots` + optional `--ca`; verification failures fatal naming endpoint/handler in `crates/spacestorage/src/client/` (FR-030)
- [ ] T055 [US3] Implement `validate <config> [--set …]` using `crates/config` only (no socket); list all problems; print effective config including derived thread count in `crates/spacestorage/src/main.rs`
- [ ] T056 [US3] Implement remote commands `status`, `config`, `threads`, `buffers`, `handlers`, `reload`, `stop [--wait]` with connection-timeout errors naming address and handler in `crates/spacestorage/src/main.rs`
- [ ] T057 [US3] Ensure `reload` CLI prints changed / applied live / pending restart and exits 5 when `pending_restart` non-empty, 2 on `config_invalid` in `crates/spacestorage/src/main.rs`

**Checkpoint**: Operator can validate → start → inspect → reload → stop using only the bundled CLI (quickstart.md)

---

## Phase 6: User Story 4 - Size node buffers and see how full they are (Priority: P2)

**Goal**: Named buffer registry with configurable capacities, live reload of sizes, usage/limit-hit reporting, overflow reject/wait, warn when sum exceeds available memory

**Independent Test**: Custom sizes at start; fill a buffer under load; read usage via admin TCP, admin-http, and CLI; raise/lower capacity via reload (SC-008, SC-009, FR-038–FR-044)

### Tests for User Story 4

- [ ] T058 [P] [US4] Write buffer integration tests (defaults, unknown/out-of-range reject, limit-hit + overflow policy, raise/lower capacity including usage &gt; 100%, concurrent reload serialization, reload reject while draining) in `crates/node/tests/buffers.rs`
- [ ] T059 [P] [US4] Extend reload integration coverage for live buffer+drain changes with zero dropped connections in `crates/node/tests/reload.rs` (SC-009)

### Implementation for User Story 4

- [ ] T060 [US4] Apply configured buffer capacities at startup from `NodeConfig.buffers` onto `BufferRegistry` in `crates/node/src/lib.rs` / startup path; unknown name / out-of-range fail before bind
- [ ] T061 [US4] Wire `net.recv` / `net.send` / `request.queue` reservations into admin TCP framing and accept/request paths in `crates/node/src/handler/admin_tcp.rs` and `crates/node/src/entrypoint/listener.rs` so limit-hits increment under load
- [ ] T062 [US4] Expose buffer reports (capacity, used, ratio, limit_hits, policy, range, default, owner) identically via AdminService `buffers`/`status` and ensure CLI tables consume them in `crates/node/src/admin/mod.rs`
- [ ] T063 [US4] On reload lowering capacity below `used`: accept, stop admitting until `used + n <= capacity`, do not discard held data; raising capacity is immediate in `crates/node/src/buffer/mod.rs` + `crates/node/src/reload.rs` (FR-040)
- [ ] T064 [US4] Emit `buffers_exceed_memory` warning at startup and reload (sum capacities vs available memory via `spawn_blocking` /proc or sysctl) without failing in `crates/node/src/reload.rs` and startup path (FR-044)
- [ ] T065 [US4] Reject entire reload atomically on unknown buffer or out-of-range capacity (no partial apply) in `crates/node/src/reload.rs`

**Checkpoint**: Per-node bounded buffers are tunable and observable through all admin surfaces

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: Documentation, packaging sanity, quickstart proof, leftover edge cases

- [ ] T066 [P] Copy starter example to `docs/examples/node.conf` from `specs/001-runtime-cli-api/contracts/fixtures/node.conf` and draft `docs/configuration.md` from `contracts/config-grammar.md`
- [ ] T067 [P] Add unit tests for lexer/parser/resolve edge cases (SIZE/DURATION literals, wrong arity, bad literal) in `crates/config/src/` modules
- [ ] T068 Harden edge cases: oversubscription threads accepted; address not owned by host → bind error; cert expiry while running logs + flags entrypoint; concurrent reloads ordered; reload while draining → `invalid_state` in `crates/node/src/`
- [ ] T069 Ensure both binaries build from one workspace (`cargo build --release` produces `target/release/spacestoraged` and `target/release/spacestorage`) documenting FR-031 in `README.md` or `docs/`
- [ ] T070 Run end-to-end validation of `specs/001-runtime-cli-api/quickstart.md` (validate → start → inspect → reload → stop) and fix gaps discovered
- [ ] T071 [P] Reserve `/metrics` 404 and confirm stats names remain stable for feature `08` without exposition in this feature (`crates/node/src/handler/admin_http.rs`, `crates/node/src/stats.rs`)

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Setup — **BLOCKS** all user stories
- **User Story 1 (Phase 3)**: Depends on Foundational — no dependency on US2–US4 (uses stub handler for concurrency/drain)
- **User Story 2 (Phase 4)**: Depends on Foundational — integrates with US1 node/lifecycle/entrypoint bind; delivers admin surfaces
- **User Story 3 (Phase 5)**: Depends on Foundational + US2 admin ops/reload (CLI talks to live admin); `validate` alone needs only Foundational config crate
- **User Story 4 (Phase 6)**: Depends on Foundational buffer registry + US2 reload/admin reporting; deepens admission under load
- **Polish (Phase 7)**: Depends on desired user stories being complete

### User Story Dependencies

- **US1 (P1)**: After Foundational — independently testable as runnable process
- **US2 (P1)**: After Foundational (+ practical dependency on US1 bind/lifecycle) — independently testable admin entrypoints
- **US3 (P2)**: After US2 for remote commands; `validate` can ship earlier against Foundational
- **US4 (P2)**: After buffer registry (Foundational) + reload/admin (US2); independently testable buffer behavior

### Within Each User Story

- Tests (where included) SHOULD be written and fail before implementation
- Models/registries before services/handlers
- Handlers before CLI clients
- Core behavior before polish edge cases

### Parallel Opportunities

- Phase 1: T002–T007 in parallel after T001
- Phase 2: lexer/AST (T010–T011), admin-proto (T018–T020), buffer/stats (T022–T023) in parallel once error/model direction is set
- Phase 3: T028–T029 tests in parallel; then implementation sequential around lifecycle
- Phase 4: T037–T038 tests and T039 TLS in parallel; admin_tcp (T042) and admin_http (T043) in parallel after AdminService (T041)
- Phase 5: T050–T051 and client TCP/HTTP (T052–T053) in parallel after args skeleton
- Phase 6: T058–T059 tests in parallel; then capacity/admission wiring
- After Foundational: US1 and config/CLI `validate` portions can proceed while US2 admin handlers are built by another developer

---

## Parallel Example: User Story 2

```bash
# Launch US2 tests together:
Task: "Write admin parity tests in crates/node/tests/admin_parity.rs"
Task: "Write TLS/plaintext entrypoint tests in crates/node/tests/tls.rs"

# After AdminService exists, launch both handlers together:
Task: "Implement admin TCP handler in crates/node/src/handler/admin_tcp.rs"
Task: "Implement admin-http axum router in crates/node/src/handler/admin_http.rs"
```

## Parallel Example: User Story 3

```bash
# Launch CLI scaffolding together:
Task: "Implement exit codes in crates/spacestorage/src/exit.rs"
Task: "Implement human/JSON output in crates/spacestorage/src/output.rs"
Task: "Implement TCP AdminClient in crates/spacestorage/src/client/tcp.rs"
Task: "Implement HTTP AdminClient in crates/spacestorage/src/client/http.rs"
```

---

## Implementation Strategy

### MVP First (User Stories 1 + 2)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL — blocks all stories)
3. Complete Phase 3: User Story 1 (runnable sized node + drain)
4. Complete Phase 4: User Story 2 (admin entrypoints — ties for MVP per spec)
5. **STOP and VALIDATE**: SC-001–SC-006, independent tests for US1 and US2
6. Demo: start node, curl `/v1/status`, stop

### Incremental Delivery

1. Setup + Foundational → foundation ready
2. US1 → sized runtime MVP process
3. US2 → remotely administrable node (complete P1 MVP)
4. US3 → bundled operator CLI (SC-007)
5. US4 → observable bounded buffers (SC-008/SC-009)
6. Polish → docs + quickstart proof (SC-011)

### Parallel Team Strategy

1. Team completes Setup + Foundational together
2. Once Foundational is done:
   - Developer A: User Story 1 (runtime/lifecycle)
   - Developer B: User Story 2 (admin handlers/TLS) after entrypoint bind from A lands
   - Developer C: CLI `validate` against `crates/config` early; remote commands after US2
3. User Story 4 overlays buffer admission once registries + reload exist

---

## Notes

- [P] tasks = different files, no dependencies on incomplete sibling tasks
- [USn] label maps task to user story for traceability
- Tests included because plan.md explicitly specifies unit, integration, contract, and CLI test locations
- Spec fixtures under `contracts/fixtures/invalid/` drive SC-004 contract coverage
- Interim auth is `admin.token_file` only; role system is feature `07`
- Metric exposition is feature `08`; this feature only tracks figures and reserves names
- Commit after each task or logical group; stop at any checkpoint to validate independently
