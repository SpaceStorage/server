---
description: "Task list for cluster identity, discovery, join, leave, and replace"
---

# Tasks: Cluster Identity, Discovery, Join, Leave, and Replace

**Input**: Design documents from `/specs/011-identity-membership/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md), [data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: Requested. Plan Testing section + Independent Tests + SC-001–SC-010 + [quickstart.md](quickstart.md). Unit tests in `crates/membership`; in-process 1/3-node harness in `crates/conformance`; contract tests on `contracts/fixtures/`. Write failing tests first where listed under a story.

**Scope of this feature**: New library crate `crates/membership` (`spacestorage-membership`) owns bootstrap/join/pending/token/drain/decommission/replace **procedures** and is the only writer of membership `ClusterStore` events. Applied membership stays in `006`/`004` `ClusterStore`. Internode framing/heartbeats stay `012`; rebalance algorithms stay `004`; Raft internals stay `006`. No new binary or client port.

**Sibling crates** (extend, do not fork): `crates/internode`, `crates/placement`, `crates/controlplane`, `crates/config`, `crates/node`, `crates/admin-proto`, `crates/spacestorage`, `crates/release-profile`, `crates/conformance`.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: [US1], [US2], [US3] on user-story phase tasks only
- Every task includes an exact file path

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Workspace member and `membership` crate skeleton per [plan.md](plan.md) Project Structure

- [ ] T001 Create `crates/membership/Cargo.toml` (package `spacestorage-membership`, edition 2024) and `crates/membership/src/lib.rs` that `mod`s `error`, `identity`, `secret`, `bootstrap`, `join`, `token`, `drain`, `decommission`, `replace`, `events`, `metrics` and exports `MembershipService`
- [ ] T002 Add `crates/membership` to workspace `[workspace.members]` in `Cargo.toml` and declare deps already used in-workspace (`tokio`, `async-trait`, `serde`/`serde_json`, `bytes`, `tracing`, `uuid`, `parking_lot`, `subtle` for constant-time secret compare, `getrandom`/`rand` for ids); no new Raft library
- [ ] T003 [P] Create empty module stubs `crates/membership/src/{error,identity,secret,bootstrap,join,token,drain,decommission,replace,events,metrics}.rs` so the crate compiles as a library before story work

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Shared types, identity directory, secret verify, ClusterStore event enums, config directives, and seams every story needs. No user-story work until this phase is complete.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [ ] T004 Implement membership errors in `crates/membership/src/error.rs` covering at least `NotMember`, `Pending`, `LiveReplace`, `RetiredIdentity`, `NameInUse`, `Ladder`, `SecretMismatch`, `SecretRotateInProgress`, `LastMember`, `DecommissionBlocked`, `NotPending`, `InvalidState`, `Minority`, `BootstrapAndJoin`, `BootstrapForeignSeeds`, `JoinSecretRequired` (codes align with [contracts/](contracts/))
- [ ] T005 [P] Implement local identity directory load-or-create in `crates/membership/src/identity.rs`: `{data_dir}/identity/node.json` fields `node_id` (UUID, random on first start, never changes) and `node_name` (from `node { name; }`, unique among **current** members); `{data_dir}/identity/cluster.json` fields `cluster_uuid` (immutable), `cluster_name` (label only), `secret_epochs`; fsync via `spawn_blocking`
- [ ] T006 [P] Implement join-secret epochs in `crates/membership/src/secret.rs`: `SecretEpoch { epoch: u64 monotonic, secret: opaque bytes, accepted: bool }`; verify is constant-time over all `accepted=true` epochs; bootstrap creates ≥32-byte secret, writes `cluster.token_file` mode 0600, records epoch 1 accepted
- [ ] T007 [P] Define cluster-scoped event types in `crates/membership/src/events.rs` exactly as [data-model.md](data-model.md) §10: `Bootstrap`, `PendingJoin`, `AdmitMember`, `MemberUpdate`, `Drain`, `Undrain`, `RemoveMember`, `RetireIdentity`, `ReplaceMember`, `SecretRotateBegin`, `SecretRotateComplete`, `JoinTokenMint`, `JoinTokenConsume`; this crate is the only writer
- [ ] T008 [P] Stub metric increments in `crates/membership/src/metrics.rs` for closed names from [contracts/metrics.md](contracts/metrics.md): `spacestorage_membership_members`, `spacestorage_membership_pending`, `spacestorage_membership_join_total{result}`, `spacestorage_membership_replace_total{result}`, `spacestorage_membership_decommission_total{result}`, `spacestorage_membership_secret_epoch` — do not rename `08` series
- [ ] T009 Extend `cluster { }` parsing/validation in `crates/config` per [contracts/config-directives.md](contracts/config-directives.md): exclusive `bootstrap` / `join`; `seeds` (alias `peers`); `token_file` = join secret; optional `join_token_file`; `secret_max_overlap` default `24h`; `join_token_ttl` default `12h` range `1h`–`72h`; codes `bootstrap_and_join`, `bootstrap_or_join_required`, `bootstrap_foreign_seeds`, `join_secret_required`, `join_token_ttl_range`
- [ ] T010 Implement `MembershipService` skeleton and `on_start` hook surface in `crates/membership/src/lib.rs` (async; called from `crates/node` before advertising `ready`); wire crate into `crates/node` dependency graph without enabling story logic yet
- [ ] T011 In `crates/controlplane`, ensure applied `MemberRecord.status` is only `ready` \| `draining` (remove unused `joining`); add apply paths for pending map / retired set / incarnation so membership events from T007 can land; pending MUST NOT enter voter set or `members` map ([contracts/membership-store.md](contracts/membership-store.md))
- [ ] T012 [P] Add additive internodes message type stubs in `crates/internode` for `JoinRequest`, `JoinAck`, `PendingAnnounce`, `FenceIncarnation`, `SecretRotate` per [contracts/internodes-membership.md](contracts/internodes-membership.md); unknown types remain ignored; join secret verify gate stays before processing

**Checkpoint**: `cargo check -p spacestorage-membership` succeeds; config fixtures under `specs/011-identity-membership/contracts/fixtures/invalid/` parse to documented codes; controlplane no longer stores `joining`. User stories can start.

---

## Phase 3: User Story 1 - Bootstrap the first node (Priority: P1) 🎯 MVP

**Goal**: Explicit bootstrap with empty/self-only seeds and **no** local cluster identity creates cluster UUID + join secret, sole member `ready`, initial controller. Same name on two isolated empty disks → two UUIDs (no merge). Restart with bootstrap still declared keeps the same UUID. Process not in membership, not bootstrapping, no reachable seed → MUST NOT become `ready` as a member (FR-008). Local cluster identity present → ignore bootstrap (FR-020).

**Independent Test**: Bootstrap one node; record UUID and secret; restart with bootstrap still declared and confirm same UUID; second isolated bootstrap same name on empty disk → different UUID (SC-001, SC-002).

### Tests for User Story 1 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T013 [P] [US1] Add unit tests in `crates/membership/src/bootstrap.rs` (or `crates/membership/tests/bootstrap.rs`) for: bootstrap creates UUID+secret+one member; second bootstrap on fresh dir different UUID; bootstrap ignored when `cluster.json` already exists; `bootstrap_foreign_seeds` refused
- [ ] T014 [P] [US1] Add conformance tests in `crates/conformance/tests/membership_bootstrap.rs` covering SC-001/SC-002 using [contracts/fixtures/bootstrap.conf](contracts/fixtures/bootstrap.conf): one-node `ready` < 10 s bound; restart keeps UUID; isolated second bootstrap different UUID and no merge
- [ ] T015 [P] [US1] Add config contract tests in `crates/config/tests/membership_directives.rs` (or equivalent) that [bootstrap-and-join.conf](contracts/fixtures/invalid/bootstrap-and-join.conf) → `bootstrap_and_join` and [bootstrap-foreign-seeds.conf](contracts/fixtures/invalid/bootstrap-foreign-seeds.conf) → `bootstrap_foreign_seeds`

### Implementation for User Story 1

- [ ] T016 [US1] Implement explicit bootstrap in `crates/membership/src/bootstrap.rs`: require `cluster { bootstrap; }`, empty or self-only seeds, **and** no local `cluster.json`; create UUID + secret epoch 1; append `Bootstrap` event; become sole voter member with `status=ready`, `incarnation=1`
- [ ] T017 [US1] Wire bootstrap path through `MembershipService::on_start` in `crates/membership/src/lib.rs` and `crates/node` so first binary reaches `ready` as sole member; if local cluster identity exists, ignore bootstrap declaration and start as that cluster (FR-005/FR-020)
- [ ] T018 [US1] On bootstrap, write join secret to `cluster.token_file` (create if missing) with mode 0600 and persist `identity/cluster.json` + `identity/node.json` via `crates/membership/src/identity.rs` / `secret.rs`
- [ ] T019 [US1] Apply `Bootstrap` in `crates/controlplane` cluster group so `snapshot().members` has exactly one member and minority attempts return `Minority { group: cluster }` with no membership change
- [ ] T020 [US1] Enforce FR-008 in `crates/membership` + `crates/node` lifecycle: process not in membership, not bootstrapping, and no reachable seed MUST NOT become `ready` as a member (does not apply to already-admitted members)
- [ ] T021 [US1] Ensure two isolated bootstraps with the same `cluster_name` produce independent UUIDs with no merge protocol in `crates/membership/src/bootstrap.rs` (FR-002)

**Checkpoint**: SC-001/SC-002 conformance passes; invalid bootstrap fixtures fail as coded; one-node MVP demo works from [quickstart.md](quickstart.md) §1.

---

## Phase 4: User Story 2 - Join a second node (Priority: P1)

**Goal**: First join presents secret + node identity + unique name + every topology-ladder key. Without token → **pending join** (not member: no vote, not replica target, not `ready` as member) until `CLUSTER_ADMIN` admit of a **still-connected** pending process. Valid unused one-time token bound to **node name only** (TTL default 12 h, range 1–72 h, single use) records membership without a second admit. Stolen secret alone MUST NOT add a member. Join does not move existing replicas. Name reusable after decommission among current members; retired identity on first join refused.

**Independent Test**: Bootstrap A; start B secret-only → pending; admit B; omit ladder key → refuse; secret without admit/token → not replica target; join C with token → no second admit; stop pending → admit refused; later start is new first join (SC-003, SC-004, SC-009).

### Tests for User Story 2 ⚠️

- [ ] T022 [P] [US2] Add unit tests in `crates/membership/src/{join,token,secret}.rs` for: pending ≠ member; token TTL/single-use/name bind; name uniqueness among current members; retired-id refuse; secret overlap verify; admit refused when pending process disconnected
- [ ] T023 [P] [US2] Add conformance tests in `crates/conformance/tests/membership_join.rs` covering SC-003/SC-004/SC-009 with [join-pending.conf](contracts/fixtures/join-pending.conf), [join-token.conf](contracts/fixtures/join-token.conf), [join-missing-ladder.conf](contracts/fixtures/invalid/join-missing-ladder.conf)
- [ ] T024 [P] [US2] Add contract fixture assertions in `crates/conformance` or `crates/config` that missing ladder creates **no** pending row and wrong/expired/reused token refuses with audit line

### Implementation for User Story 2

- [ ] T025 [P] [US2] Implement first-join handshake in `crates/membership/src/join.rs`: after secret+labels accepted, no token → append `PendingJoin` (fields per [data-model.md](data-model.md) §4); valid token → `AdmitMember` without second admit; process exit drops pending; admit of disconnected pending → refuse
- [ ] T026 [P] [US2] Implement one-time tokens in `crates/membership/src/token.rs`: `JoinToken` bound to `node_name` only for token life; optional `node_id`; `expires_at`; `used: bool`; default TTL 12 h (1–72 h); mint/consume events; mismatched name → refuse
- [ ] T027 [US2] Wire `JoinRequest` / `JoinAck` (`pending` \| `admitted` \| `replaced` \| `refused` + code) and `PendingAnnounce` in `crates/internode` + `crates/membership/src/join.rs`; non-members may send only `JoinRequest` after secret verify ([contracts/internodes-membership.md](contracts/internodes-membership.md))
- [ ] T028 [US2] On `AdmitMember`, insert into `members` with `status=ready` and adjust voter/learner set per research R12 in `crates/controlplane` (1 voter → third member expands to 3; further members learners); pending remains excluded from voters and replica targets
- [ ] T029 [US2] Call `004` placement ladder APIs from `crates/membership/src/join.rs` so omit/integrity failure → `JoinAck.refused` code `ladder` and **no** pending row; join MUST NOT move existing replica placements
- [ ] T030 [US2] Implement admin ops `Membership`, `Admit { node_id }`, `JoinTokenMint { node_name, node_id?, ttl? }` in `crates/admin-proto` (+ node handlers) per [contracts/admin-cli.md](contracts/admin-cli.md); first-binary auth: admin bearer = `CLUSTER_ADMIN`
- [ ] T031 [US2] Add CLI verbs in `crates/spacestorage`: `membership`, `admit <node-id>`, `join-token mint --name <node-name> [--id <uuid>] [--ttl 12h]` with `--output json` and exit codes from contract
- [ ] T032 [US2] In `crates/placement`, treat non-members and pending joins as **not** new replica targets and **not** voters (FR-007/FR-009); members remain eligible coordinators (FR-014)
- [ ] T033 [US2] Implement secret rotate begin/complete in `crates/membership/src/secret.rs` + admin/CLI `cluster secret-rotate begin|complete` and internodes `SecretRotate`; overlap until `complete` or `secret_max_overlap` (default 24h) drops old epoch
- [ ] T034 [US2] Enforce retired-identity refuse on first join and name-unique-among-current-members in `crates/membership/src/join.rs` (FR-018); name reuse after decommission accepted only with **new** `node_id`

**Checkpoint**: Pending vs token paths pass SC-003/004/009; ladder/secret failures create no member; admit-after-exit refused.

---

## Phase 5: User Story 3 - Drain, decommission, and replace (Priority: P1)

**Goal**: Operator drain marks `draining`, stops new tenant connections and new placements, **process stays up**; undrain → `ready`; stop signal drains then exits (`001`). Live decommission copies/re-places from still-running draining node then `RemoveMember`+`RetireIdentity`; last remaining member decommission refused (including data-loss accept). Replace only when `012` FD marks unavailable; heartbeating → `live_replace`; success increments incarnation and fences old process. Already-admitted restart needs persisted membership + join secret only (no new admit/token; seeds not a restart gate).

**Independent Test**: Three-node RF=2; operator-drain (process up) + decommission; replace after FD timeout on fresh data dir with same identity; restart all three without new admit; refuse last-member decommission (SC-005–SC-008, SC-010).

### Tests for User Story 3 ⚠️

- [ ] T035 [P] [US3] Add unit tests in `crates/membership/src/{drain,decommission,replace}.rs` for: drain vs stop (`exit_after_drain`); last-member refuse; incarnation fence; live_replace when heartbeating; retired id cannot be replaced
- [ ] T036 [P] [US3] Add conformance tests in `crates/conformance/tests/membership_lifecycle.rs` covering SC-005/SC-006/SC-007/SC-008/SC-010: drain/undrain/live decommission; replace after timeout + fence; restart without seeds; name reuse vs retired identity; last-member decommission refused

### Implementation for User Story 3

- [ ] T037 [US3] Implement operator drain/undrain in `crates/membership/src/drain.rs`: `Drain` → `status=draining`, no new tenant accept, no new placements, process stays up; `Undrain` → `ready`; distinguish from `001` stop path (`exit_after_drain: true`)
- [ ] T038 [US3] Implement decommission in `crates/membership/src/decommission.rs`: require draining if process reachable; call `004` rebalance; blockers → `DecommissionBlocked { containers[] }` with process still up; else `RemoveMember` + `RetireIdentity`; **refuse** when target is last remaining member even with `accept_data_loss` (FR-011/FR-019); voter adjust per R12 (3→2 → one voter + one learner)
- [ ] T039 [US3] Implement replace in `crates/membership/src/replace.rs`: allow iff `012` marks `node_id` unavailable now; heartbeating `ready`/`draining` → `LiveReplace`; on success `incarnation += 1`, emit `FenceIncarnation`; placements keep naming the id; new process may use fresh data dir presenting existing `node_id` via `JoinRequest.replace_of`
- [ ] T040 [US3] In `crates/node`, wire operator drain ≠ stop: admin drain keeps process running; SIGTERM/`Stop` still drain-then-exit; rolling restart uses stop-signal path only (FR-010/FR-015)
- [ ] T041 [US3] In `crates/placement`, exclude `draining` from **new** replica targets while existing replicas remain until rebalance/decommission; decommission orchestration waits on rebalance plan completion or data-loss accept
- [ ] T042 [US3] Implement admin ops `Drain`, `Undrain`, `Decommission { node_id, accept_data_loss }`, `Replace { node_id }` in `crates/admin-proto` + node handlers per [contracts/admin-cli.md](contracts/admin-cli.md) and [contracts/drain-decommission-replace.md](contracts/drain-decommission-replace.md)
- [ ] T043 [US3] Add CLI verbs in `crates/spacestorage`: `drain`, `undrain`, `decommission [--accept-data-loss]`, `replace <node-id>` with documented exit codes (`live_replace`, `DecommissionBlocked`, minority retryable)
- [ ] T044 [US3] Implement already-admitted restart in `crates/membership` + `crates/node`: persisted membership + join secret only → eligible for `ready`; empty seeds OK; leftover bootstrap ignored; no peer reachable → MAY still `ready` from local membership (FR-016/SC-007)
- [ ] T045 [US3] Enforce fenced incarnation on `internode`/`replication` in `crates/internode`: stale incarnation → close stream; must not vote or be replica target after replace commits

**Checkpoint**: SC-005–SC-008 and SC-010 pass; operator drain keeps process up; last member cannot be decommissioned; replace fences old incarnation.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Metrics, interim audit, release-profile flag, quickstart validation, cleanup

- [ ] T046 [P] Finish metric increments at join/admit/replace/decommission/secret-rotate sites in `crates/membership/src/metrics.rs` with closed `result` label sets from [contracts/metrics.md](contracts/metrics.md)
- [ ] T047 [P] Emit interim audit records (admit, token mint/use/refuse, decommission, replace, secret-rotate) to in-memory ring + `membership.audit` tracing in `crates/membership` until `014` durable audit ships (research R15)
- [ ] T048 Enable membership in first-binary release profile wiring in `crates/release-profile` / `crates/node` so slices 1–5 include bootstrap/join (plan First binary)
- [ ] T049 [P] Align operator docs/examples with [quickstart.md](quickstart.md) and copy/reference fixtures under `docs/examples/` (or documented path) for bootstrap, join-pending, join-token
- [ ] T050 Run [quickstart.md](quickstart.md) steps 1–7 against in-process/loopback harness and ensure `cargo test -p spacestorage-membership` plus `crates/conformance/tests/membership_*.rs` pass SC-001–SC-010
- [ ] T051 [P] `rustfmt` / `clippy` clean pass on `crates/membership` and new conformance/config tests

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Setup — **BLOCKS** all user stories
- **User Story 1 (Phase 3)**: Depends on Foundational — no dependency on US2/US3 🎯 MVP
- **User Story 2 (Phase 4)**: Depends on Foundational; practically needs US1 bootstrap for multi-node demos but pending/token logic is independently testable with a fixture cluster
- **User Story 3 (Phase 5)**: Depends on Foundational; needs membership mutations from US1/US2 for three-node drain/replace scenarios
- **Polish (Phase 6)**: Depends on desired user stories being complete

### User Story Dependencies

- **US1 (P1) Bootstrap**: After Phase 2 only — MVP gate
- **US2 (P1) Join**: After Phase 2; integrates with US1 cluster UUID/secret but stories remain independently testable via harness
- **US3 (P1) Drain/decommission/replace**: After Phase 2; uses admit/token from US2 for three-node setup; restart-without-seeds (FR-016) can be validated once US1 membership persistence exists

### Within Each User Story

- Tests (listed) MUST be written and FAIL before implementation
- Models/events before services; services before admin/CLI/internode wiring
- Story complete before treating the next priority as done

### Parallel Opportunities

- Phase 1: T003 parallel with T002 after T001 skeleton exists
- Phase 2: T005–T008 and T012 can run in parallel after T004 codes exist
- US1 tests T013–T015 in parallel; then implementation T016→T021
- US2 tests T022–T024 in parallel; T025/T026 in parallel before T027–T034
- US3 tests T035–T036 in parallel; T037–T039 in parallel before node/placement/admin wiring
- Once Foundational completes, US1 can ship as MVP while US2/US3 proceed on separate tracks if staffed

---

## Parallel Example: User Story 1

```bash
# Tests in parallel (fail first):
Task: "Unit tests bootstrap in crates/membership/src/bootstrap.rs"
Task: "Conformance SC-001/002 in crates/conformance/tests/membership_bootstrap.rs"
Task: "Config fixtures bootstrap-and-join / bootstrap-foreign-seeds"

# Then implementation sequentially from T016:
Task: "Implement bootstrap.rs"
Task: "Wire on_start + persist identity/secret"
```

## Parallel Example: User Story 2

```bash
Task: "Unit tests join/token/secret"
Task: "Conformance membership_join.rs SC-003/004/009"
Task: "Implement join.rs and token.rs in parallel"
```

## Parallel Example: User Story 3

```bash
Task: "Unit tests drain/decommission/replace"
Task: "Conformance membership_lifecycle.rs SC-005–008/010"
Task: "Implement drain.rs, decommission.rs, replace.rs in parallel"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL)
3. Complete Phase 3: User Story 1 (bootstrap + FR-008/FR-020)
4. **STOP and VALIDATE**: SC-001/SC-002 + quickstart §1
5. Demo one-node `ready` cluster with UUID + join secret

### Incremental Delivery

1. Setup + Foundational → foundation ready
2. US1 → bootstrap MVP
3. US2 → pending + token join (three-node path for `016` slice 5)
4. US3 → drain/decommission/replace + restart-without-seeds
5. Polish → metrics, audit ring, quickstart SC-001–SC-010 green

### Parallel Team Strategy

1. Team completes Setup + Foundational together
2. After Foundational:
   - Dev A: US1 bootstrap
   - Dev B: US2 join/token (against stubbed ClusterStore if needed)
   - Dev C: US3 drain/replace types + tests
3. Integrate on shared `MembershipService` + controlplane apply

---

## Notes

- [P] = different files, no incomplete-task dependencies
- [USn] maps to spec user stories for traceability
- Tests included because plan Testing + SC-001–SC-010 + quickstart explicitly require them
- Commit after each task or logical group
- Stop at any checkpoint to validate the story independently
- Do not put MI6/firewall, tenant WAL, or Raft election internals in this crate
