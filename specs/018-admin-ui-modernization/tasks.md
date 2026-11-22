---
description: "Task list for Modern Admin Web Console"
---

# Tasks: Modern Admin Web Console

**Input**: Design documents from `/specs/018-admin-ui-modernization/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md), [data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: Required per [spec.md](spec.md) User Scenarios & Testing, [plan.md](plan.md) Testing, and [quickstart.md](quickstart.md). Write listed conformance/unit tests first; they must fail before the implementation they guard. No browser-driver toolchain.

**Scope**: Extend `crates/admin-ui` (`spacestorage-admin-ui`) with a server-rendered shell (`maud` + checked-in `shell.js` / `shell.css`). Cross-feature seams in `crates/authz`, `crates/admin-proto`, `crates/membership`, `crates/node` (`admin_http.rs`). `009` map/console composers stay in `map.rs` / `console.rs`; shell links via `/ui/map` and `/ui/browse`. UI copy and tests use SpaceStorage nouns (namespace, container, shard, replica, job, principal) — not Elasticsearch product nouns. Pagination: `limit` default **50**, max **100**; container descriptor scan cap **10 000** per request.

**Sibling crates** (consume, do not redefine): `001` admin HTTP, `014` authz/session, `007` tenancy, `006`/`011` membership, `008` metrics, `010` jobs, `004`/`012` placement, `009` map/console, `015` limits.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: [US1]–[US6] on user-story phase tasks only
- Every task includes an exact file path

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Module skeleton, assets, and workspace wiring per [plan.md](plan.md) project structure

- [X] T001 Add shell module tree in `crates/admin-ui/src/lib.rs`: `mod shell`, `overview`, `namespaces`, `nodes`, `monitoring`, `jobs_view`; export a `ui_shell_router()` beside the existing map/console router
- [X] T002 [P] Create stub modules `crates/admin-ui/src/shell.rs`, `crates/admin-ui/src/overview.rs`, `crates/admin-ui/src/namespaces.rs`, `crates/admin-ui/src/nodes.rs`, `crates/admin-ui/src/monitoring.rs`, `crates/admin-ui/src/jobs_view.rs` with `todo!()` composers and documented contract pointers
- [X] T003 [P] Add checked-in assets `crates/admin-ui/src/assets/shell.js` and `crates/admin-ui/src/assets/shell.css` (`include_str!` in `crates/admin-ui/src/pages.rs` or `shell.rs`); no npm, no CDN URLs
- [X] T004 [P] Add shared pagination/limit types in `crates/admin-ui/src/shell.rs`: parse `limit` default 50, reject outside 1–100 with `limit_exceeded` per [research.md](research.md) R5; opaque `cursor` helpers; close a page when the next row would exceed **256 KiB** serialized JSON and set `next_cursor` (never drop rows without a cursor); stable sort — namespaces and nodes by name, jobs by id, audit by `AuditEntry.id` descending
- [X] T005 [P] Add bounded descriptor scan helper in `crates/admin-ui/src/namespaces.rs`: cap **10 000** `ContainerCatalog` descriptors per request; drop catalog lock before any `.await` per [research.md](research.md) R6
- [X] T006 Copy contract fixtures into test references: assert against `specs/018-admin-ui-modernization/contracts/fixtures/overview-degraded.json`, `namespace-page.json`, `audit-page.json` from `crates/admin-ui/tests/` or `crates/conformance/tests/`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Session cookie, CSRF, new `AdminOp`s, membership drain path, shell HTTP contract, security headers. **No user story work until this phase completes.**

**⚠️ CRITICAL**: Blocks all user stories

### Tests (foundational)

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [X] T007 [P] Add conformance module `crates/conformance/tests/ui_session_auth.rs` covering [quickstart.md](quickstart.md) §1–4: unauthenticated `/ui/overview` HTML (no `/v1/ui/*` URLs as immediate fetches), login `Set-Cookie` `ss_session` with `HttpOnly; Secure; SameSite=Strict`, JSON has `csrf_token` and **no** `token`, `GET /ui` → **308** `/ui/overview`, `/ui/cluster` → `/ui/map`, `/ui/console` → `/ui/browse` (query preserved)
- [X] T008 [P] Extend `crates/conformance/tests/ui_session_auth.rs`: cookie `POST /v1/admin` without `Origin` → **403** `csrf_rejected`; foreign `Origin` and wrong `X-SpaceStorage-CSRF` → **403**; matching `Origin`+CSRF runs; CLI `Authorization: Bearer` admin token still works without CSRF; `Authorization: Bearer` session hex → **401** `session_invalid`; cookie `POST /v1/console/query` (or `/v1/console/config`) without CSRF → **403** `csrf_rejected` with no console side effect
- [X] T009 [P] Add unit tests in `crates/authz/tests/session_csrf.rs`: `SessionToken` stores `csrf_token` (32-byte hex, not the session secret); stable across GET; revoked on logout

### Implementation (foundational)

- [X] T010 Add `csrf_token` field to session record in `crates/authz/src/` (mint at `SessionTokenStore::issue`, constant-time compare helper, clear on `revoke_hash`) per [contracts/session-cookie.md](contracts/session-cookie.md)
- [X] T011 Implement cookie issuance on `POST /v1/auth/login` in `crates/node/src/handler/admin_http.rs`: `Set-Cookie: ss_session=<hex>; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=43200`; JSON body includes `principal`, `principal_id`, `scope`, `namespaces`, `expires_at`, `expires_in`, `csrf_token`, `affordances` — **no** `token` field
- [X] T012 [P] Implement `GET /v1/auth/session` and `POST /v1/auth/logout` in `crates/node/src/handler/admin_http.rs`: session probe returns affordances; logout requires cookie+Origin+CSRF, revokes hash, clears cookie
- [X] T013 Implement admin-http middleware in `crates/node/src/handler/admin_http.rs`: resolve `ss_session` cookie; for mutating routes require matching `Origin` (host+port = `Host`; `http` only for `localhost`/`127.0.0.1`) and `X-SpaceStorage-CSRF`; reject session hex as `Authorization: Bearer`; preserve admin-token bearer without CSRF per [contracts/session-cookie.md](contracts/session-cookie.md)
- [X] T014 [P] Add `AdminOp::AuditList { cursor, limit, namespace }` to `crates/admin-proto/src/ops.rs` and JSON (de)serialization; `limit` default 50, max 100
- [X] T015 [P] Add `AdminOp::Drain { node_id }` and `AdminOp::Undrain { node_id }` to `crates/admin-proto/src/ops.rs` per [contracts/drain-admin.md](contracts/drain-admin.md)
- [X] T016 Implement `audit_list` handler in `crates/node/src/handler/admin_http.rs` (or `crates/authz`): gate `CLUSTER_ADMIN` or `AUDIT_READ`; namespace filter; page by `AuditEntry.id` descending; `limit`/`cursor` R5; no secrets in response per [contracts/audit-list.md](contracts/audit-list.md)
- [X] T017 Implement `MembershipService::operator_drain` / `operator_undrain` in `crates/membership/src/` delegating to `membership::drain` / `undrain` in `crates/membership/src/drain.rs`; wire `AdminOp::Drain` / `Undrain` in `crates/node/src/handler/admin_http.rs` (`CLUSTER_ADMIN` only)
- [X] T018 [P] Register permanent **308** redirects and section routes in `crates/admin-ui/src/lib.rs` per [contracts/shell-http.md](contracts/shell-http.md): `/ui`→`/ui/overview`, `/ui/cluster`→`/ui/map`, `/ui/console`→`/ui/browse`; serve `shell.js`, `shell.css` under `/ui/assets/{name}`
- [X] T019 Add CSP, `nosniff`, and `Referrer-Policy: same-origin` on shell HTML, `/v1/ui/*`, `/v1/auth/*`, and `/ui/assets/*` in `crates/admin-ui/src/lib.rs` or `crates/node/src/handler/admin_http.rs` per [contracts/shell-http.md](contracts/shell-http.md) (no `unsafe-inline`)
- [X] T020 Refactor `crates/admin-ui/src/pages.rs` to maud shell document: `data-path` on `<body>`, landmarks (`header`, `nav`, `main`), login-only document when unauthenticated (no nav to data sections), noscript limited-interactivity sentence
- [X] T021 Remove session token from `sessionStorage` in `crates/admin-ui/src/assets/console.js` and `crates/admin-ui/src/assets/cluster-map.js`; attach cookie session + `X-SpaceStorage-CSRF` on all `POST /v1/console/*` mutations; set `window.SS_SHELL = true` before map/console scripts load per [research.md](research.md) R3; add failing static checks (until removed) that `console.js` and `cluster-map.js` contain no `sessionStorage` session writes
- [X] T022 [P] Add `affordances` derivation from `014` verbs in `crates/authz/src/` and include in login/session JSON per [data-model.md](data-model.md) UI Session View

**Checkpoint**: Cookie login, CSRF gate, redirects, and new admin ops compile; `cargo test -p spacestorage-conformance --test ui_session_auth` exercises §1–4 (may still fail on shell polish until US1).

---

## Phase 3: User Story 1 — Sign in to a coherent admin shell (Priority: P1) 🎯 MVP

**Goal**: Single shell with persistent navigation (Overview, Namespaces, Nodes, Monitoring, Jobs, Audit, Cluster map, Browse). Identity, scope, expiry visible. No cluster payload before session. Cookie-only credential; mutating routes CSRF-protected. SpaceStorage nav labels per [contracts/shell-http.md](contracts/shell-http.md).

**Independent Test**: [spec.md](spec.md) US1 — load UI without session (login only); sign in as cluster admin (full nav); namespace admin (narrowed scope); foreign `Origin` write refused; sign out clears cookie; expired session returns to login with `next` preserved; no script-readable session.

### Tests for User Story 1 ⚠️

- [X] T023 [P] [US1] Unit test in `crates/admin-ui/tests/shell_html.rs`: unauthenticated `GET /ui/namespaces` HTML contains no tenant names, no embedded namespace JSON, and landmark `nav` links only after session probe path in `shell.js`
- [X] T024 [P] [US1] Unit test in `crates/admin-ui/tests/shell_vocabulary.rs`: navigation labels are Overview, Namespaces, Nodes, Monitoring, Jobs, Audit, Cluster map, Browse — assert absence of primary-copy Elasticsearch product nouns `index`, `document`, `ILM`, `data stream` in user-visible strings in `crates/admin-ui/src/pages.rs`, `shell.rs`, `crates/admin-ui/src/assets/shell.js`, `cluster-map.js`, and `console.js`
- [X] T025 [P] [US1] Conformance in `crates/conformance/tests/ui_session_auth.rs`: after login, `GET /v1/auth/session` returns `scope` and `namespaces`; `POST /v1/auth/logout` then session probe **401** `session_invalid`; static analysis of `shell.js`, `cluster-map.js`, and `console.js` finds no `sessionStorage` write of a session credential

### Implementation for User Story 1

- [X] T026 [US1] Implement section dispatch and nav affordance filtering in `crates/admin-ui/src/shell.rs`: sections `overview`, `namespaces`, `nodes`, `monitoring`, `jobs`, `audit`, `map`, `browse`; omit nav when affordance false; direct load shows refusal without data
- [X] T027 [P] [US1] Implement login flow in `crates/admin-ui/src/pages.rs` and `crates/admin-ui/src/assets/shell.js`: `POST /v1/auth/login`, then `GET /v1/auth/session`; honor safe relative `next` starting with `/ui/` only per [contracts/session-cookie.md](contracts/session-cookie.md)
- [X] T028 [US1] Render shell chrome in `crates/admin-ui/src/pages.rs`: signed-in principal, effective scope (cluster vs namespace), session expiry; sign-out control calling `POST /v1/auth/logout` with `Origin` and `X-SpaceStorage-CSRF`
- [X] T029 [P] [US1] Implement `crates/admin-ui/src/assets/shell.js` bootstrap: no `/v1/ui/*` fetch until session **200**; attach `X-SpaceStorage-CSRF` on all cookie-authenticated mutations; on **401** `session_invalid` replace view with login preserving `next` and in-progress form field values without marking the form as submitted
- [X] T030 [US1] Mount authenticated section `GET` handlers in `crates/admin-ui/src/lib.rs` for all `/ui/overview` … `/ui/browse` paths; unauthenticated → login document per [contracts/shell-http.md](contracts/shell-http.md)
- [X] T031 [P] [US1] Implement namespace-scoped shell narrowing in `crates/admin-ui/src/shell.rs`: cluster-wide sections absent or disabled for namespace admins; **403** explanatory refusal for forbidden direct links without foreign tenant payload

**Checkpoint**: US1 independent test passable on loopback; MVP demo = sign in and navigate empty shell sections.

---

## Phase 4: User Story 2 — Cluster overview and health at a glance (Priority: P1)

**Goal**: `GET /v1/ui/overview` Cluster Overview Snapshot; degraded members and lagging replicas above the fold; panel age and stale after 30 s; poll 10 s via `shell.js` only. Namespace admins receive a **filtered** overview for their permitted namespace(s) — **200**, not **403** for the whole route.

**Independent Test**: [spec.md](spec.md) US2 — three-node degraded fixture; pause refresh; tab hidden stops polls.

### Tests for User Story 2 ⚠️

> **NOTE: T034 MUST fail before T038 poll implementation**

- [X] T032 [P] [US2] Unit tests in `crates/admin-ui/tests/overview_composer.rs` against [contracts/fixtures/overview-degraded.json](contracts/fixtures/overview-degraded.json): `members.degraded[]` and `replicas.named[]` capped at 20 with `*_omitted` flags; `source` `controller` vs `node-local`
- [X] T033 [P] [US2] Conformance test in `crates/conformance/tests/ui_overview.rs`: not-ready member and lagging replica named in overview JSON; single-node roster absent still lists local node; namespace admin `GET /v1/ui/overview` returns **200** with counts and degradations scoped to their namespace only (no cluster-wide tenant data)
- [X] T034 [P] [US2] Failing-first unit tests in `crates/admin-ui/tests/shell_poll.rs` guarding [quickstart.md](quickstart.md) §5: `shell.js` uses a single **10 s** interval poller with single-flight per panel; pause control stops scheduled polls; panel marks stale when `generated_at` age exceeds **30 s**; **zero** poll requests scheduled while `document.hidden` (instrument via test doubles or request counter hooks)

### Implementation for User Story 2

- [X] T035 [US2] Implement overview composer in `crates/admin-ui/src/overview.rs`: panels `readiness`, `members`, `controller`, `counts`, `replicas`, `jobs`; `generated_at`; per-panel `status` `ok`|`unavailable`; 1.5 s budget → **504** `deadline_exceeded` per [contracts/ui-read-models.md](contracts/ui-read-models.md)
- [X] T036 [P] [US2] Wire `counts.containers` via bounded scan (T005) and `counts.namespaces` from in-memory `007` list length; set `container_count_complete` when scan hits cap; for namespace-scoped principals, filter all panels to permitted namespaces server-side
- [X] T037 [US2] Register `GET /v1/ui/overview` on admin-http in `crates/admin-ui/src/lib.rs` with server-side tenant filter: cluster admin gets cluster-wide snapshot; namespace admin gets namespace-filtered snapshot (**200**, not whole-route **403**)
- [X] T038 [P] [US2] Implement overview panel rendering in `crates/admin-ui/src/assets/shell.js` and `shell.css`: degraded callouts above the fold with text labels (not color alone); show relative and RFC3339 age; mark stale when age > 30 s
- [X] T039 [US2] Implement poll contract in `crates/admin-ui/src/assets/shell.js`: **10 s** interval, one in-flight per panel, pause control, no poll while `document.hidden` per [contracts/shell-http.md](contracts/shell-http.md)

**Checkpoint**: Overview loads within quickstart §5; stale/pause behavior testable.

---

## Phase 5: User Story 3 — Namespace lifecycle management (Priority: P1)

**Goal**: Paged namespace list with container count and quota; create, rename, delete (cascade), quota replace via `POST /v1/admin`; namespace detail with read-only container inventory and links to `/ui/map` and `/ui/browse`.

**Independent Test**: [spec.md](spec.md) US3 — create/rename/cascade delete; namespace admin isolation; `limit=101` → `limit_exceeded`.

### Tests for User Story 3 ⚠️

- [X] T040 [P] [US3] Unit tests in `crates/admin-ui/tests/namespaces_page.rs` against [contracts/fixtures/namespace-page.json](contracts/fixtures/namespace-page.json): `limit` 50 default; `container_count_complete: false` copy path "at least N"; **256 KiB** page boundary sets `next_cursor` without silent row drop
- [X] T041 [P] [US3] Conformance in `crates/conformance/tests/ui_namespaces.rs`: create appears without full reload (re-fetch list); delete without cascade → `CascadeRequired`; cascade confirm per [quickstart.md](quickstart.md) §6.5 (names namespace + container count, not typed ack); quota replace refused with `007` `message` leaves displayed quota unchanged; quota replace after confirm updates summary only on **200**; rename success makes old direct URL resolve to renamed namespace or explicit not-found, never a different tenant; `GET /v1/ui/namespaces/other` as namespace admin → **403** empty of foreign data

### Implementation for User Story 3

- [X] T042 [US3] Implement `GET /v1/ui/namespaces` in `crates/admin-ui/src/namespaces.rs`: `q` filter, R5 pagination (T004), tenant-filtered rows per [data-model.md](data-model.md) Namespace View
- [X] T043 [US3] Implement `GET /v1/ui/namespaces/{name}` with paged `containers` inventory (descriptor metadata only, no rows) and map-derived `shards` / `replicas_degraded` or JSON `null`
- [X] T044 [P] [US3] Implement namespaces section UI in `crates/admin-ui/src/assets/shell.js`: empty state with create action; server validation `message` beside fields; cascade delete uses **normal** confirmation that names the namespace and states container count (use "at least" when incomplete) per [quickstart.md](quickstart.md) §6.5 — not typed acknowledgement
- [X] T045 [US3] Wire namespace mutations through existing `AdminOp` paths with cookie+CSRF in `shell.js` (`namespace_create`, `rename`, `delete`, `quota_replace`); update list only after successful GET refresh (no optimistic success)

**Checkpoint**: Namespace lifecycle quickstart §6–§6.5 passable.

---

## Phase 6: User Story 4 — Node inventory and node detail (Priority: P2)

**Goal**: Paged nodes list with pending joins; detail with threads/buffers/labels for local node; namespace admin sees only replica hosts.

**Independent Test**: [spec.md](spec.md) US4 — filters `q` and `status` in URL; unknown readiness when reporter unreachable.

### Tests for User Story 4 ⚠️

- [X] T046 [P] [US4] Unit tests in `crates/admin-ui/tests/nodes_composer.rs`: `readiness` `pending` for pending joins; `unknown` never coerced to `ready`; peer `worker_threads` null
- [X] T047 [P] [US4] Conformance in `crates/conformance/tests/ui_nodes.rs`: namespace admin node list ⊆ map hosts for their namespace; cluster admin sees pending join

### Implementation for User Story 4

- [X] T048 [US4] Implement `GET /v1/ui/nodes` in `crates/admin-ui/src/nodes.rs` with `q`, `status`, R5 pagination, tenant host filter from `009` map snapshot per [data-model.md](data-model.md) Node View
- [X] T049 [US4] Implement `GET /v1/ui/nodes/{id}` with `state`, `worker_threads`, `buffers`, `labels`, `reported_by`; null → UI "unavailable"
- [X] T050 [P] [US4] Implement nodes section in `crates/admin-ui/src/assets/shell.js`: filter query sync to URL; list+detail navigation without full document reload

**Checkpoint**: Nodes quickstart §7 (node portion) passable.

---

## Phase 7: User Story 5 — Monitoring review and job progress (Priority: P2)

**Goal**: `GET /v1/ui/monitoring` renders `008` series unchanged; unavailable series listed not zeroed; paged jobs with progress.

**Independent Test**: [spec.md](spec.md) US5 — metric names match registry; namespace admin cannot request cluster scope.

### Tests for User Story 5 ⚠️

- [X] T051 [P] [US5] Unit tests in `crates/admin-ui/tests/monitoring_composer.rs`: absent family in `unavailable[]`, not `value: 0`; label sets unmodified
- [X] T052 [P] [US5] Conformance in `crates/conformance/tests/ui_jobs.rs`: running migration shows progress fields; namespace admin `GET /v1/ui/monitoring` without namespace → **403**

### Implementation for User Story 5

- [X] T053 [US5] Implement `GET /v1/ui/monitoring` and `GET /v1/ui/monitoring/namespaces/{name}` in `crates/admin-ui/src/monitoring.rs` reading in-process `008` registry per [contracts/ui-read-models.md](contracts/ui-read-models.md)
- [X] T054 [P] [US5] Implement `GET /v1/ui/jobs` and `GET /v1/ui/jobs/{id}` in `crates/admin-ui/src/jobs_view.rs` wrapping `010` with R5 pagination
- [X] T055 [US5] Implement monitoring and jobs panels in `crates/admin-ui/src/assets/shell.js` with stale age display and 10 s poll when section visible

**Checkpoint**: Monitoring/jobs quickstart §7 passable.

---

## Phase 8: User Story 6 — Guarded operational actions (Priority: P3)

**Goal**: Confirmations for reload, quota, jobs, drain, undrain, cluster stop; audit read-only page; strong confirmations for drain and stop; `audit_list` only source.

**Independent Test**: [spec.md](spec.md) US6 — dismissed dialog sends no POST; audit refused without `AUDIT_READ`; drain dialog empty type-in field.

### Tests for User Story 6 ⚠️

- [X] T056 [P] [US6] Conformance in `crates/conformance/tests/ui_drain_audit.rs` against [contracts/drain-admin.md](contracts/drain-admin.md) and [contracts/audit-list.md](contracts/audit-list.md): drain/undrain CSRF refusal leaves membership status unchanged; namespace admin `op: drain` → **403**; confirmed drain as cluster admin sets targeted member to `draining` and process stays up; confirmed undrain returns member to `ready`; dismiss drain/undrain confirmation sends no matching `POST /v1/admin`; `audit_list` page has no `export`/`retention` keys; fixture [contracts/fixtures/audit-page.json](contracts/fixtures/audit-page.json)
- [X] T057 [P] [US6] HTML assertion tests in `crates/admin-ui/tests/guarded_actions.rs`: drain dialog impact sentence and empty acknowledgement `value`; cluster stop requires typing `cluster`; grep confirms no pre-filled drain target
- [X] T058 [P] [US6] Conformance in `crates/conformance/tests/ui_guarded_writes.rs`: after reload confirmation, outcome message reflects server accept or refuse; job cancel and resume report success state from `010` on **200** and show refusal `message` on **403**/**4xx** without optimistic UI update

### Implementation for User Story 6

- [X] T059 [US6] Implement `GET /v1/ui/audit` in `crates/admin-ui/src/shell.rs` (or dedicated module) proxying `audit_list` with R5; render-only table in `shell.js` — no export/retention controls
- [X] T060 [P] [US6] Implement shared confirmation framework in `crates/admin-ui/src/assets/shell.js` (generalizes namespace dialogs from T044): `strength` `normal` vs `strong`; strong actions (`node_drain`, `cluster_stop`) require typed target; dismiss → no fetch; outcome `unconfirmed` on network drop then re-read
- [X] T061 [US6] Wire guarded writes in `shell.js` to `POST /v1/reload`, `POST /v1/stop`, `POST /v1/admin` (drain/undrain), `POST /v1/jobs/{id}/cancel|resume` with cookie+Origin+CSRF; show server `message` on refusal
- [X] T062 [P] [US6] Hide/disable controls from `affordances` only for UX; ensure routes still enforce verbs in `crates/node/src/handler/admin_http.rs` (FR-003)

**Checkpoint**: quickstart §8–9 passable for audit, drain, stop.

---

## Phase 9: Map/browse hosting and 009 integration (Cross-cutting)

**Purpose**: Shell hosts `009` views without duplicate timers or iframes; links preserve namespace query.

- [X] T063 [P] Implement `/ui/map` and `/ui/browse` shell pages in `crates/admin-ui/src/pages.rs`: mount points for `cluster-map.js` / `console.js`; namespace picker when map namespace unset; `GET /v1/cluster/map?namespace=` only for allowed namespace
- [X] T064 Refactor `crates/admin-ui/src/assets/cluster-map.js` to export one-shot render entry; remove private `setInterval` when `window.SS_SHELL` per [research.md](research.md) R3; user-visible headings use container/shard/replica
- [X] T065 [P] Wire shell-hosted browse mount in `crates/admin-ui/src/pages.rs` and `crates/admin-ui/src/assets/console.js`: namespace detail deep links to `/ui/browse?namespace=` and `/ui/map?namespace=` only (console `POST` CSRF remains on T021)

---

## Phase 10: Polish & Cross-Cutting Concerns

**Purpose**: Accessibility, metrics, observability, quickstart validation, 014 doc reconciliation note

- [X] T066 [P] WCAG-oriented styles in `crates/admin-ui/src/assets/shell.css`: `:focus-visible`, contrast pairs, non-color status text classes for ready/not_ready/pending/unknown/stale/failed
- [X] T067 [P] Increment `spacestorage_api_*` with `application="spacestorage-ui"` and `protocol="admin-http"` on shell and `/v1/ui/*` handlers in `crates/node/src/handler/admin_http.rs` per [contracts/shell-http.md](contracts/shell-http.md)
- [X] T068 [P] HTML structure tests in `crates/admin-ui/tests/a11y_shell.rs`: `main` landmark, table `th`, form `label` associations on login, overview, namespace create, node detail flows; layout smoke at **768 px** width and **200%** zoom for overview, namespace create, and node detail (no horizontal scroll on primary controls — complement [quickstart.md](quickstart.md) §10 human pass when no harness)
- [X] T069 Run [quickstart.md](quickstart.md) §1–9 via `cargo test -p spacestorage-conformance` and crate unit tests; record §10 keyboard/air-gap as human pass once on loopback
- [X] T070 [P] Reconcile `specs/014-authz-keys/plan.md` and `spec.md` with cookie session + `audit_list` (documentation only, no behavior change) per cross-feature note in [plan.md](plan.md)

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: No dependencies — start immediately
- **Phase 2 (Foundational)**: Depends on Phase 1 — **blocks all user stories**
- **Phase 3 (US1)**: Depends on Phase 2 — **MVP**
- **Phase 4 (US2)**: Depends on Phase 2 + US1 shell/router (T026–T030); poll tests (T034) before T039
- **Phase 5 (US3)**: Depends on Phase 2 + US1 (mutations need CSRF shell)
- **Phase 6 (US4)**: Depends on Phase 2 + US1; uses map snapshot for tenant node filter (T048 after map composer available)
- **Phase 7 (US5)**: Depends on Phase 2 + US1
- **Phase 8 (US6)**: Depends on Phase 2; audit/drain handlers (T016–T017); best after US1 affordances (T022)
- **Phase 9 (Map/browse)**: Depends on US1 shell; can parallel US4–US6 once US1 lands
- **Phase 10 (Polish)**: Depends on desired user stories complete

### User Story Dependencies

| Story | Priority | Depends on | Independent test |
|-------|----------|------------|------------------|
| US1 | P1 | Foundational | Login-only gate, nav, CSRF, cookie |
| US2 | P1 | US1 shell | Overview JSON + stale/poll |
| US3 | P1 | US1 | Namespace CRUD + tenant isolation |
| US4 | P2 | US1 (+ map for filter) | Nodes list/detail + filters |
| US5 | P2 | US1 | Metrics fidelity + jobs page |
| US6 | P3 | Foundational ops + US1 | Confirmations + audit + drain |

### Parallel Opportunities

- Phase 1: T002–T006 in parallel after T001
- Phase 2: T007–T009 tests parallel; T010–T022 many `[P]` across authz, admin-proto, membership, admin-ui
- After Phase 2: US2–US5 composers (different files) can proceed in parallel once US1 shell (T026–T030) exists
- Phase 9 (T063–T065) parallel with US4–US6 if map/console ownership boundaries respected

### Parallel Example: User Story 1

```text
T023 shell_html.rs ∥ T024 shell_vocabulary.rs ∥ T025 conformance session probes
Then T027 login JS ∥ T031 namespace narrowing
```

---

## Implementation Strategy

### MVP First (User Story 1)

1. Complete Phase 1 + Phase 2 (cookie, CSRF, redirects, admin ops)
2. Complete Phase 3 (US1 shell + session UX)
3. **STOP and VALIDATE** US1 independent test + quickstart §1–4
4. Add US2 (overview) as first data panel

### Incremental Delivery

1. Foundation → US1 (navigable secure shell)
2. US2 + US3 (P1 operator value: health + namespaces)
3. US4 + US5 (depth: nodes + monitoring/jobs)
4. US6 + Phase 9 (day-two ops + map/browse hosting)
5. Phase 10 polish and conformance gate `cargo test -p spacestorage-conformance`

---

## Notes

- Tasks marked `[P]` touch different files or are tests with no ordering inside the same file section
- Do not mark task `[X]` until implementation and its tests pass
- Do not edit `spec.md` or `plan.md` during implement; `014` reconciliation (T070) is docs-only exception named in plan
- Avoid Elasticsearch product nouns in UI copy, tests, and confirmation strings

---

## Phase 11: Convergence

**Goal**: Close gaps between the shipped shell and the spec, plan, and contracts. Server-side verb checks come first. UI tasks render data the composers already return, and align strong confirmations with `drain-admin.md`.

- [X] T071 CRITICAL: Refuse a namespace-scoped session, before any state change, on cluster-scoped admin writes and on reads that would disclose another tenant — `namespace_list` / `namespace_create` / `namespace_rename` / `namespace_delete` / `quota_replace`, other cluster admin ops (`reload`, `stop`, membership, voter, join-token, role lists), `POST /v1/reload`, `POST /v1/stop`, and `POST /v1/jobs/{id}/cancel|resume`. Admin-token bearer stays allowed. Set `affordances_for_session` so `namespace_write`, `quota_replace`, `config_reload`, `cluster_stop`, `node_drain`, and `job_control` are true only for `cluster_admin` (`SessionToken` has no `MIGRATE` grant; do not add one) per FR-003, SC-003, US6/AC2, US6/AC7, `data-model.md` affordances (contradicts)
- [X] T072 Stop the shell-hosted map from starting `cluster-map.js`'s private `setInterval`. That script runs before `shell.js`, and the inline `window.SS_SHELL=true` is blocked by `script-src 'self'`, so `/ui/map` always polls on its own sub-10s timer. Detect the shell from server-rendered markup, export the one-shot render, and let the shell call it per FR-010, plan R3 (contradicts)
- [X] T073 Render the overview snapshot the composer already returns: ready and not-ready member counts, namespace and container counts, replica health summary, and active, failed, and recently completed job counts, with the load time per FR-007, US2/AC1 (partial)
- [X] T074 Label overview figures with `source` (`controller` or `node-local`) and controller availability so node-local data is not shown as cluster-wide truth per FR-026, US2/AC3 (partial)
- [X] T075 Re-evaluate each panel's `generated_at` while refresh is paused and when a hidden tab becomes visible, and mark the panel stale once the age exceeds 30 s without waiting for the next fetch per FR-009, US2/AC4 (partial)
- [X] T076 Show each namespace's quota summary on the Namespaces list (name and container count are already shown) per FR-011 (partial)
- [X] T077 Require a quota-replace confirmation that names the namespace and the proposed quota, and do not send `quota_replace` until that confirmation per FR-011a, US3/AC9 (partial)
- [X] T078 Show controller role on each Nodes list row per FR-017, US4/AC1 (partial)
- [X] T079 Show node detail state, controller role, topology labels, and `reported_by`, and render nulls as unavailable per FR-018, US4/AC3 (partial)
- [X] T080 Open Monitoring on the principal's namespace series: a namespace admin must not land on `GET /v1/ui/monitoring` (that route is 403 for them), and a cluster admin must be able to open a per-namespace monitoring view from the section per US5/AC1, US5/AC5 (partial)
- [X] T081 Show job kind, target, status, progress, and failure on the jobs list and job detail per FR-020, US5/AC3 (partial)
- [X] T082 Surface a paused job (`010` keeps `status` as `running` and sets `paused`) and offer resume for it; cancel and resume confirmations must name the job and its target per FR-020a, US6/AC8 (partial)
- [X] T083 Make drain a strong confirmation on the selected member: the dialog names that node's name and id, states that it stops new tenant connections and placements and that replicas move off before decommission, starts with an empty type-in, and requires the node name. `POST` body stays `{ "op": "drain", "node_id" }` per FR-021a, US6/AC6, `contracts/drain-admin.md` (contradicts)
- [X] T084 State in the cluster-stop confirmation that every service on the cluster stops, still require typing `cluster`, and keep the type-in empty per FR-021a, US6/AC7 (partial)
- [X] T085 Show audit `time` in the audit table, or "time unavailable" when `time` is null, and say there are no entries when the page is empty per US6/AC3, `contracts/audit-list.md` (partial)
- [X] T086 Render namespace, container, node, metric, job, and audit strings as text in `shell.js` (escape before `innerHTML`, as `cluster-map.js` already does) per FR-023 (contradicts)
- [X] T087 Truncate very long names, labels, and error text in the shell layout and keep the full value available, so primary flows do not gain a horizontal scroll at tablet width per the long-name edge case, NFR-005 (partial)

---

## Phase 12: Convergence

**Goal**: Close gaps that remain after Phase 11. Tenant isolation on admin reads comes first. The shell then has to age every visible panel, page lists, and let a namespace admin open monitoring.

- [X] T088 CRITICAL: Refuse a namespace-scoped session, before the handler reads tenancy state, on `AdminOp::NamespaceDescribe` for a name outside the session's scopes and on `AdminOp::Leases` for every namespace-scoped caller. The refusal is 403 with no namespace record, container list, or lease set, and it does not say whether the foreign namespace exists. Describing a namespace the session holds may proceed. Admin-token and cluster-admin callers stay allowed per FR-016, US3/AC6, CC-004 (contradicts)
- [X] T089 Show each visible shell panel's age from its `generated_at` and mark it stale after 30 s without waiting for the next fetch, including while refresh is paused and before a visibility refetch returns. Overview age is baked into `#overview-panel` and `markSectionAge` only updates `#section-panel`, so overview, namespaces, nodes, and audit never flip to stale. Do not leave the hosted map labeled live after that age per FR-009, US2/AC4, US2/AC5, `contracts/shell-http.md` poll (partial)
- [X] T090 Call the hosted map's one-shot render from the shell's single 10 s poller while `/ui/map` is the visible section, still with no private `setInterval` in `cluster-map.js`. `pollableSection` skips the map, so the shell never refreshes it per plan R3, FR-010, `contracts/shell-http.md` (partial)
- [X] T091 Keep Monitoring in the nav for a namespace admin (`metrics_read` is true) and open that principal's namespace series at `/ui/monitoring/namespaces/{name}` instead of hiding the link and instead of calling `GET /v1/ui/monitoring` per US5/AC5 (partial)
- [X] T092 Follow `next_cursor` on the namespaces, nodes, jobs, and audit lists so a row past the first page is reachable, without requesting an unbounded collection per FR-011, FR-021b, US6/AC3, large-fleet edge (partial)
- [X] T093 Show job progress and failure on the jobs list as well as on job detail per FR-020, US5/AC3 (partial)
- [X] T094 Report a failed, timed-out, or unavailable UI read in the affected panel with the server's reason, and leave navigation usable. `loadSection` returns without a message on any status other than 401 or 403 per FR-022, SC-009 (partial)
- [X] T095 Resolve an undrain target the same way as drain (a name or id on the loaded list becomes `node_id`) and state in the confirmation that the node may accept placements again per `contracts/drain-admin.md` (partial)

---

## Phase 13: Convergence

**Goal**: Make cursors reach the next page, and stop a paused or failed panel from being presented as live.

- [X] T096 Emit `next_cursor` on `GET /v1/ui/nodes` when another row remains after the page. `compose_nodes_list` truncates the sort keys to the page and then requires `sort_keys.len() > page.len()`, so the cursor is always null and a node past the first page is unreachable per large-fleet edge, `contracts/ui-read-models.md` nodes, T092 (partial)
- [X] T097 Advance the jobs list cursor to the following page. `compose_jobs_list` sorts ids descending and `page_start` keeps the first key greater than the cursor, so the next request repeats the first page or returns none per `contracts/ui-read-models.md` jobs, T092 (partial)
- [X] T098 Page the namespace detail container inventory: honor `cursor` on `GET /v1/ui/namespaces/{name}`, set `containers.next_cursor` when more containers remain, and follow that cursor in the shell so a container past the first page is reachable per large-fleet edge, FR-015, `contracts/ui-read-models.md` namespace detail (partial)
- [X] T099 When the hosted map's data is older than 30 s, including while refresh is paused and before a visibility refetch returns, stop rendering the banner heading "Cluster live". The age line goes stale; `cluster-map.js` leaves that heading until the next render per FR-009, US2/AC4, US2/AC5 (contradicts)
- [X] T100 Put the pause-refresh control on Namespaces, Nodes, Audit, and the hosted map. Those sections poll every 10 s and their documents have no `#poll-pause`, so the operator cannot pause them per FR-009, `contracts/shell-http.md` poll (partial)
- [X] T101 When a UI read's fetch fails before an HTTP status (network drop), show that failure in the affected panel and leave navigation usable. `loadSection` handles a non-OK status and has no handler for a rejected fetch per FR-022, SC-009, `contracts/shell-http.md` errors (partial)

## Phase 14: Convergence

**Goal**: Keep refresh pausable after the first paint, and do not call the hosted map live unless a map payload was just loaded.

- [X] T102 Do not rewrite the hosted map banner heading to "Cluster live" unless that render loaded map data. `ensureMapRenderHook` stamps `lastMapAt` and `syncMapStatusHeading` replaces the `#map-status` h2 after every `SS_SHELL_MAP_RENDER`, including a failed fetch, a session refusal, and the namespace picker, so those banners become "Cluster live". A successful load must still flip to stale after 30 s, including while refresh is paused and before a visibility refetch returns per FR-009, US2/AC4, US2/AC5, T099 (contradicts)
- [X] T103 Keep `#poll-pause` on Namespaces, Nodes, and Audit after the section paints. The button sits inside `#section-mount`, and `renderNamespaces`, `renderNodes`, and `renderAudit` replace that element's `innerHTML`, so the control is gone after the first successful load while those sections keep polling every 10 s. The hosted map already keeps its button per FR-009, US2/AC4, T100 (partial)