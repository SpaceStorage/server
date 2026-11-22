# Implementation Plan: Modern Admin Web Console

**Branch**: `018-admin-ui-modernization` | **Date**: 2026-11-22 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/018-admin-ui-modernization/spec.md` (approved 2026-11-22). Clarifications from Session 2026-10-31 and Session 2026-11-22 are encoded in the spec. Two sizing questions deferred by the spec are decided in [research.md](research.md) R5 and R6.

## Summary

Replace the current admin HTML entry (`/ui` → `/ui/console`, plus `/ui/cluster`) with one **server-rendered shell** on the existing `admin-http` listener. The shell speaks only SpaceStorage nouns — cluster, namespace, container, shard, replica, job, principal — and covers Overview, Namespaces, Nodes, Monitoring, jobs, and audit, with guarded writes at full CLI parity.

Cerebro and Kibana are a **capability class**, not a visual or vocabulary target. This feature owns the management shell. `009` keeps the cluster-map graphic (`GET /v1/cluster/map`) and the configure/browse console (`POST /v1/console/*`). The shell links to those views under `/ui/map` and `/ui/browse`; `/ui/cluster` and `/ui/console` permanently redirect into the shell. Ingest stays `009`.

Three cross-feature seams are in this plan's scope and do not redefine the owner's semantics:

1. `014` session issuance becomes an `HttpOnly; Secure; SameSite=Strict` cookie plus `Origin` and a server-bound CSRF proof on cookie-authenticated mutations (R4).
2. A read-only paginated `audit_list` admin operation, gated by `AUDIT_READ` or `CLUSTER_ADMIN` (R7).
3. `AdminOp::Drain` and `AdminOp::Undrain` call the existing `011` membership functions (R8).

Pages are `maud` HTML plus checked-in JS/CSS in `crates/admin-ui`. No npm, no CDN, no second process. The shell is the only poller: 10 s interval, stale after 30 s.

## Technical Context

**Language/Version**: Rust edition 2024, `rust-version` 1.88 (workspace `Cargo.toml`). Same Cargo workspace as `001`–`016`.

**Primary Dependencies**: existing workspace crates (`tokio`, `axum`/`hyper`, `serde`/`serde_json`, `maud` 0.26 already used by `crates/admin-ui`, `uuid`, `sha2` via `spacestorage-authz`). No new non-Rust dependency. No Node.js, npm, bundler, or CDN fetch.

**Storage**: No new store. Session hashes stay in the `014` `SessionTokenStore`. Audit rows are the `014` `AuditEntry` collection (cluster-log source of truth when that apply path is present; this feature only adds a paginated read). Namespace, membership, placement, and catalog truth stay in `006`/`007`/`011`/`004`/`types`. Container counts are computed by a bounded descriptor scan (R6), not a maintained counter.

**Testing**: `cargo test`. Unit tests in `crates/admin-ui`, `crates/authz`, and `crates/admin-proto`. Conformance in `crates/conformance` for cookie issuance, CSRF refusal, server-side tenant filter, paginated `audit_list`, drain/undrain admin ops, and shell redirects. HTML assertions for landmarks, labels, and confirmation copy. No browser-driver toolchain.

**Target Platform**: Linux x86_64/aarch64 servers (primary), macOS for development. The console is served by every node that has `admin-http` (constitution VI). Browser support: current and previous major evergreen browsers, viewport ≥ 768 px, 200% zoom (NFR-005).

**Project Type**: Extension of the existing library crate `crates/admin-ui`, plus small admin-path additions in `crates/admin-proto`, `crates/authz`, `crates/membership`, and `crates/node`. No new binary and no new listen port.

**Performance Goals**: Overview shows current status within 2 s for 95% of loads on the reference cluster (3 nodes, 50 namespaces, 500 containers, 5 000 shards, one migration). Table and filter interactions within 1 s p95 (NFR-001). One poll per visible panel per 10 s; zero polls while the tab is hidden (FR-010). UI read handlers give up at 1.5 s with `deadline_exceeded` rather than holding a worker (NFR-004).

**Constraints**: Rust-only (CC-001). Async Tokio; no fsync and no catalog lock held across `.await` (CC-002). Same process as admin HTTP (CC-003). Server-side namespace filter (CC-004). Render `008` series unchanged (CC-005). No second membership or config store (CC-006). Closed `014` verbs; cookie session; no script-readable session secret (CC-007). No second query engine (CC-008). Listing pages: default 50 rows, hard max 100, JSON ≤ 256 KiB, under `015` `max_result` 64 MiB and under the `009` console cap of 1 000 rows / 1 MiB (R5). Descriptor scan cap 10 000 per request (R6).

**Scale/Scope**: Operator tool, about ten concurrent browsers (NFR-002). Large-fleet edge the UI must paginate: 500 namespaces, 5 000 containers, 1 000 nodes. Shell sections: Overview, Namespaces, Nodes, Monitoring, Jobs, Audit, Cluster map (link to `009`), Browse (link to `009`). Roughly 4–6 k lines in `admin-ui` shell plus tests, and thin admin-op handlers elsewhere.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Gate for this feature | Status |
|---|-----------|------------------------|--------|
| I | Rust-Only Stack | Shell, composers, and admin ops are Rust. `maud` is already a Rust dependency. JS/CSS are checked-in assets served by the binary, not a Cargo or npm dependency. No CDN. | PASS |
| II | Fully Asynchronous Tokio Runtime | UI handlers are async on the admin-http runtime. Catalog descriptor walks take a short read lock and release it before any `.await`. No fsync on a worker. Read budget 1.5 s. Scan cap 10 000 descriptors (R6). | PASS |
| III | Single-Process Multithreaded Monolith | Routes mount on the existing `admin-http` router inside `spacestoraged`. No sidecar, proxy, or web tier. | PASS |
| IV | Type-Driven Multiparadigm | The shell is a presentation layer. It does not add an L0–L4 type or a storage engine. Container inventory reads `Container` descriptors; it does not browse rows. | PASS |
| V | Protocol Compatibility on Distinct Ports | No new listen port and no new client protocol. The console is operator HTML on `admin-http`. | PASS |
| VI | Every Node Is a Request Coordinator | Any node with `admin-http` serves the shell. Overview figures that need the controller say so when the controller is unreachable, and fall back to the local node on a single-node cluster. | PASS |
| VII | Label-Based Planetary Placement | Node detail shows the topology labels the membership and placement surfaces already report. The shell does not invent a placement domain. `planet` stays a label. | PASS |
| VIII | Cassandra-Style Quorum | Unchanged. The shell does not ack writes. Namespace, quota, job, drain, and stop calls go to the owning surface, which already applies quorum. | PASS |
| IX | Multi-Tenant Namespaces | Every list, count, metric, and node set is filtered in the handler from the `014` principal. A namespace admin's payload contains only their namespace. Direct requests for another namespace return 403 with no body that discloses contents. | PASS |
| X | Observability as a Product Surface | Monitoring panels render `008` family names and labels as stored. UI traffic increments the existing `spacestorage_api_*` series with `application="spacestorage-ui"` and `protocol="admin-http"`. No new required label name. | PASS |
| XI | Documented, Expandable Configuration | No new config block. Starter paths are the existing `admin-http` base URL. Unknown node states and job kinds render as the raw identifier. | PASS |
| XII | Raft Controller Elections and Local Restore | Namespaces, membership, and audit reads come from the control plane / `014` log. The shell keeps no configuration replica. Node-local status is labelled node-local when the controller view is absent. | PASS |
| XIII | Security Defaults for Data and Roles | Authorization uses the closed `014` verb set. Session secret is an `HttpOnly; Secure; SameSite=Strict` cookie. Mutating cookie requests check `Origin` and a server-bound CSRF token before any state change. Audit of privileged actions stays on the owning surface. | PASS |
| Arch. Contracts | Adapter ≠ type system | Read models call admin, tenancy, membership, jobs, metrics, and the `009` map composer. `POST /v1/console/query` remains the only ad-hoc execution path and still goes through `005`. | PASS |
| Observability Contract | Intent `08` labels | Additive use of existing series only. Required `008` labels are not renamed or dropped. | PASS |

**Gate result (pre-research)**: PASS. Checked-in browser assets are recorded in Complexity Tracking, the same justification `009` already accepted. No principle is waived.

## Project Structure

### Documentation (this feature)

```text
specs/018-admin-ui-modernization/
├── plan.md                          # This file
├── research.md                      # Phase 0 (R1–R12)
├── data-model.md                    # Phase 1 view models
├── quickstart.md                    # Phase 1 validation guide
├── contracts/
│   ├── shell-http.md                # /ui routes, redirects, CSP, poll
│   ├── session-cookie.md            # 014 cookie, Origin, CSRF
│   ├── ui-read-models.md            # /v1/ui/* composed reads
│   ├── audit-list.md                # AdminOp::AuditList
│   ├── drain-admin.md               # AdminOp::Drain / Undrain
│   └── fixtures/
│       ├── README.md
│       ├── overview-degraded.json
│       ├── namespace-page.json
│       └── audit-page.json
├── checklists/requirements.md
└── tasks.md                         # Phase 2 (/speckit-tasks — not created here)
```

### Source Code (repository root)

```text
crates/
├── admin-ui/                                # spacestorage-admin-ui — shell + existing 009 map/console
│   └── src/
│       ├── lib.rs                           # router: shell routes, redirects, /v1/ui/*
│       ├── pages.rs                         # maud shell + login document
│       ├── shell.rs                         # section dispatch, affordances, poll contract
│       ├── overview.rs                      # Cluster Overview Snapshot composer
│       ├── namespaces.rs                    # paged namespace + container inventory
│       ├── nodes.rs                         # roster + detail, tenant host filter
│       ├── monitoring.rs                    # 008 series view, no renamed metrics
│       ├── jobs_view.rs                     # paged job progress
│       ├── map.rs                           # 009 — unchanged composer
│       ├── console.rs                       # 009 — unchanged query/config path
│       └── assets/
│           ├── shell.js                     # sole poller, CSRF header, confirmations
│           ├── shell.css                    # one light theme, focus, non-color status
│           ├── cluster-map.js               # 009 graphic; shell calls render, no private timer
│           ├── console.js                   # 009 browse/configure; same
│           ├── console.css
│           └── ui.css
├── authz/                                   # SessionToken gains csrf_token; revoke on logout
├── admin-proto/                             # AdminOp::AuditList, Drain, Undrain
├── membership/                              # operator_drain / operator_undrain → drain.rs
├── node/                                    # cookie auth, Origin/CSRF gate, compose wiring
│   └── src/handler/admin_http.rs
└── conformance/                             # cookie, CSRF, pagination, redirects, drain
```

**Structure Decision**: Keep one UI crate. `009` map and console modules stay the implementation of the graphic and the browse/configure console. New shell modules sit beside them and call them. HTTP authentication changes live in `crates/node` next to the existing login handler, because that process owns `SessionTokenStore` and the admin router. `crates/admin-ui` does not gain its own session store.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| Checked-in vanilla JS and CSS in `crates/admin-ui/src/assets/` | Polling, confirmation dialogs, and keyboard focus cannot be a full document reload every 10 s, and the session secret must stay out of the page. Assets are `include_str!` bytes, not crates. | A pure-HTML reload misses FR-009/FR-010 and drops in-progress form values on session expiry. An npm SPA or a CDN script violates Principle I and NFR-008/NFR-009. |

No other constitution variance. Cookie-or-admin-token authentication is a security design (R4), not a second privilege model.

## Phase 0 and Phase 1 outputs

Phase 0 resolved every Technical Context choice, including the two items the spec deferred. See [research.md](research.md).

Phase 1 contracts:

- Shell routes, permanent redirects, CSP, and the 10 s poll: [contracts/shell-http.md](contracts/shell-http.md)
- Session cookie, Origin, CSRF, CLI bearer exception: [contracts/session-cookie.md](contracts/session-cookie.md)
- Composed reads: [contracts/ui-read-models.md](contracts/ui-read-models.md)
- `audit_list`: [contracts/audit-list.md](contracts/audit-list.md)
- Drain and undrain on the admin path: [contracts/drain-admin.md](contracts/drain-admin.md)
- View fields: [data-model.md](data-model.md)
- How to prove it: [quickstart.md](quickstart.md)

## Cerebro / Kibana class on SpaceStorage entities

The combined admin web surface (this shell plus the `009` views it hosts) targets operator capability in the class of Cerebro and Kibana. The plan maps that capability onto SpaceStorage entities only. Primary UI copy uses the right-hand column. Elasticsearch product nouns (index, document, ILM, data stream) are not navigation labels, empty states, or confirmation text.

| Operator need (capability class) | SpaceStorage surface in this plan | Owner |
|----------------------------------|-----------------------------------|--------|
| Cluster health and topology | Overview: readiness, ready / not-ready / pending members, controller and Raft availability, named degradations | `018` shell, data from `001`/`006`/`011` |
| Node inventory and node detail | Nodes: identity, readiness, controller role, threads, buffers, topology labels | `018` shell, data from `001`/`011` |
| Shard / replica map | `/ui/map` renders containers, shards, replicas, placement, and lag via `GET /v1/cluster/map`. One namespace at a time when the operator has more than a page of namespaces | Graphic: `009`. Chrome and namespace picker: `018` |
| Cluster / tenant management | Overview, Namespaces, Nodes, Monitoring, Jobs, Audit, guarded writes | `018` |
| Browse stored data | `/ui/browse` and the namespace-detail link call `POST /v1/console/query` through `005` | `009` console, linked by `018` |
| Configure cluster or namespace | Same `009` console config path (`POST /v1/console/config`), linked from the shell. No private query engine | `009` |
| Ingest pipelines | Not in the shell. No ingest authoring nav | `009` ingest, out of scope |

`/ui/cluster` redirects to `/ui/map`. `/ui/console` redirects to `/ui/browse`. Bookmarks resolve. The shell does not embed those pages in an iframe (`frame-ancestors 'none'`).

## Cross-feature notes (not open questions)

`014`'s spec and plan still describe a page-held bearer token and do not document paginated `audit_list`. This plan is the design for those two deltas. Reconciling the `014` documents is a ship prerequisite already stated in the `018` spec. It is not a planning unknown and does not block `/speckit-tasks`. Session lifetime stays 12 h with no idle timeout. The permission vocabulary and `AuditEntry` fields stay `014`'s.

`011` remains the only writer of membership events. The new admin ops call `membership::drain` / `undrain` and persist the event those functions return.

## Constitution Check (post-design)

Re-checked after [research.md](research.md), [data-model.md](data-model.md), and [contracts/](contracts/).

The design adds no process, port, type hierarchy, metric name, or label. Session state remains the `014` token store; the cookie is a transport. CSRF state is a non-secret field on that same session record. Listing pages are capped below `015` `max_result`. Container counts do not add a counter that could diverge from `ContainerCatalog`. The catalog lock is not held across `.await`. Map and console execution stay on the `009` composer and on `005`. Drain status changes only through the `011` event. UI request metrics reuse `spacestorage_api_*` with the existing application and protocol labels.

**Gate result (post-design)**: PASS. Complexity Tracking is unchanged and still covers only the checked-in assets.
