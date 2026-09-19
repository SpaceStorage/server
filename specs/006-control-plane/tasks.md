---
description: "Task list for control-plane hierarchy, Raft elections, and node restore"
---

# Tasks: Control-Plane Hierarchy, Raft Elections, and Node Restore

**Input**: Design documents from `/specs/006-control-plane/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md), [data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: Requested by [plan.md](plan.md) Testing section (unit tests in `controlplane`, `crates/conformance` SC harness, contract fixture validation). Spec Independent Tests + SC-001–SC-011. Write failing tests first where listed under a story. First-binary conformance covers SC-001–SC-005, SC-007–SC-008, SC-011; SC-006 needs ordered type / `controlplane-leases`; SC-009–SC-010 need `controlplane-ops` (slice 7).

**Scope of this feature**: New crate `crates/controlplane` (`spacestorage-controlplane`) implementing `004` `ClusterStore` via in-process `openraft` on existing `internode`. Cluster + per-namespace Raft; leadership leases with epoch fence; restore orchestration invoking `013`; slice-7 voter migrate + exclusive-data. No new binary/port. Do not reimplement tenant WAL, placement quorum arithmetic, or RBAC vocabulary.

**Sibling crates** (wire seams; do not duplicate internals): `crates/internode`, `crates/placement`, `crates/catalog`, `crates/types`, `crates/durability`, `crates/config`, `crates/node`, `crates/admin-proto`, `crates/spacestorage`, `crates/release-profile`, `crates/conformance`.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: [US1]…[US5] on user-story phase tasks only
- Every task includes an exact file path

## Path Conventions

- Core: `crates/controlplane/src/`
- Additive messaging: `crates/internode/`
- Wiring: `crates/node/`, `crates/config/`, `crates/placement/`, `crates/admin-proto/`, `crates/spacestorage/`
- Proof: `crates/conformance/`

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Workspace member, crate skeleton, `openraft` dependency, feature inventory from plan (`controlplane-ops`, `controlplane-leases`)

- [ ] T001 Create `crates/controlplane/Cargo.toml` (package `spacestorage-controlplane`, edition 2024, MSRV 1.85) depending on workspace `tokio`, `async-trait`, `serde`/`serde_json`, `bytes`, `tracing`, `uuid`, `parking_lot`, and pin pure-Rust `openraft` (no `*-sys`)
- [ ] T002 Create `crates/controlplane/src/lib.rs` that declares modules `error`, `group`, `membership`, `raft_net`, `raft_store`, `apply`, `cluster`, `namespace`, `lease`, `restore`, `metrics_agg`, `read`, `ops` and exports `ControlPlane` / `RaftClusterStore` stubs
- [ ] T003 Add `crates/controlplane` to workspace `[workspace.members]` in `Cargo.toml` and declare Cargo features `controlplane-ops` (slice 7 migrate/exclusive-data) and `controlplane-leases` (ordered-type lease conformance) defaulting off for first-binary profile consumers
- [ ] T004 [P] Copy [contracts/fixtures/raft-block.conf](contracts/fixtures/raft-block.conf) into `docs/examples/controlplane/raft-block.conf` (or link from `016` starters) documenting loopback `heartbeat 50ms; election_timeout 300ms;` vs production defaults `500ms` / `2s`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Shared types, errors, disk layout, Raft network/store adapters, config parse, and internodes message stubs every story needs. No user-story work until this phase is complete.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [ ] T005 Implement `GroupId { Cluster | Namespace(Uuid) }` in `crates/controlplane/src/group.rs` with on-disk ids `cluster` and `ns/<uuid>` under `{data_dir}/raft/<group_id>/`
- [ ] T006 [P] Implement validation/error codes in `crates/controlplane/src/error.rs`: `NotLeader { group, leader }`, `Minority { group }`, `NotMember`, `VoterSetOdd { size }`, `VoterSetMajorityLost`, `StaleEpoch { have, need }`, `LeaseNotGranted { container }`, `LeaseForbidden { type }`, `ExclusiveDataBlocked { node, containers }`, `Slice7Required { op }`, `UnknownRaftFormat { version }` (refusal text names group, nodes, and what would succeed)
- [ ] T007 [P] Implement `VoterSet { group, voters, learners, epoch }` in `crates/controlplane/src/membership.rs` with invariants `voters.len() % 2 == 1`, `voters ⊆ members`, `learners ∩ voters = ∅`, and refuse even size with `VoterSetOdd`
- [ ] T008 Implement `RaftLogRecord { group, term, index, hlc, body }` and durable log+snapshot layout with format version in `crates/controlplane/src/raft_store.rs`; fsync ONLY via bounded `spawn_blocking` (constitution II)
- [ ] T009 [P] Implement `openraft` `RaftNetwork` adapter in `crates/controlplane/src/raft_net.rs` targeting internodes (no new port)
- [ ] T010 [P] Add additive internodes message types in `crates/internode` (payload crate path per existing msg registry): `RaftVote`, `RaftAppend`, `RaftSnapshot`, `RaftForward`, `MetricsPush` per [contracts/raft-rpc.md](contracts/raft-rpc.md); unknown types stay ignored
- [ ] T011 [P] Parse `cluster.raft { heartbeat; election_timeout; }` and `controller_exclusive_data` in `crates/config` per [contracts/config-directives.md](contracts/config-directives.md): defaults `heartbeat 500ms`, `election_timeout 2s`; reject `heartbeat 0`, `election_timeout 0`, and `election_timeout <= heartbeat`; first-binary profile rejects `controller_exclusive_data on` as `Slice7Required` / `unknown_directive`
- [ ] T012 Wire empty `RaftClusterStore` implementing `004` `ClusterStore` trait signatures in `crates/controlplane/src/lib.rs` (route later in apply) so `crates/placement` can depend on the type without the interim LWW shipper
- [ ] T013 [P] Add admin-proto stubs `ControllerView`, `VoterSet`, `LeaseView`, `NotLeader` in `crates/admin-proto` per [contracts/admin-cli.md](contracts/admin-cli.md) and [data-model.md](data-model.md) §8

**Checkpoint**: `cargo check -p spacestorage-controlplane` compiles with `openraft` adapters and config validation units for invalid fixtures under `specs/006-control-plane/contracts/fixtures/invalid/`. User stories can start.

---

## Phase 3: User Story 1 - Elect a cluster primary and keep a secondary (Priority: P1) 🎯 MVP

**Goal**: Dedicated odd-sized cluster voter set elects primary+secondary; cluster store holds membership, namespace list, nodes, drives, role blob; minority of voters cannot mutate; non-voters still coordinate client data; first-binary static voter growth 1→3.

**Independent Test**: Three-node cluster (all three voters): observe primary+secondary, stop primary, confirm new primary, confirm minority of a 1+1 partition cannot join a fourth node. Optionally add non-voting members and confirm metadata still progresses when a non-voter stops (SC-001, SC-002, SC-003, SC-008).

### Tests for User Story 1 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T014 [P] [US1] Add unit tests in `crates/controlplane/src/membership.rs` for first-binary voter table: 1 member → `{A}`; 2 → `{A}` + B learner; 3 → one config change `{A,B,C}`; 4+ stay `{A,B,C}`; even proposals → `VoterSetOdd`
- [ ] T015 [P] [US1] Add conformance election tests in `crates/conformance` (e.g. `tests/controlplane_election.rs`) for SC-001/SC-002: 3 in-process voters, exactly one primary + ≥1 secondary, kill primary → new primary, membership unchanged; scrape `spacestorage_leader_elections_total` / duration histogram (SC-008)
- [ ] T016 [P] [US1] Add conformance minority tests in `crates/conformance` for SC-003: partitioned minority of cluster voters refuses join / create-namespace with `Minority { group: cluster }` even if non-voters are reachable on the minority side
- [ ] T017 [P] [US1] Add config validation tests in `crates/config` that [raft-heartbeat-zero.conf](contracts/fixtures/invalid/raft-heartbeat-zero.conf), [raft-election-timeout-le-heartbeat.conf](contracts/fixtures/invalid/raft-election-timeout-le-heartbeat.conf), and [exclusive-data-on-first-binary.conf](contracts/fixtures/invalid/exclusive-data-on-first-binary.conf) exit validate with code 2

### Implementation for User Story 1

- [ ] T018 [P] [US1] Implement `ClusterState` apply machine in `crates/controlplane/src/cluster.rs`: `cluster_uuid` (immutable from `011`), `cluster_name`, `members: NodeId → MemberRecord` (`status` ∈ `joining|ready|draining|dead`), `namespaces` list only, drives/memory inventory pointers, opaque `roles` blob, cluster `voter_set`, `exclusive_data` default `false`, last applied `hlc`
- [ ] T019 [US1] Implement cluster Raft group bootstrap and election in `crates/controlplane` (group start + `openraft` runtime): first bootstrap voter set `{self}`; only voters process `RaftVote`; emit election metrics labels `node`, `raft_group=cluster` per [contracts/metrics.md](contracts/metrics.md)
- [ ] T020 [US1] Implement first-binary static voter expansion in `crates/controlplane/src/membership.rs` / apply path: on third member admit, single `ExpandToThree { b, c }` membership op; further joins are learners only; `Replace`/`Grow`/`Shrink` return `Slice7Required`
- [ ] T021 [US1] Route cluster-scoped `ClusterStore::append` commits through cluster Raft majority in `crates/controlplane/src/apply.rs` / `lib.rs`; minority → `Minority { group: cluster }`; do not count non-voters toward majority ([contracts/cluster-store.md](contracts/cluster-store.md))
- [ ] T022 [US1] Enforce `FR-010` in `crates/controlplane`: process not in membership → `NotMember`, must not vote and must not be a new replica target; membership necessary but not sufficient to vote
- [ ] T023 [US1] Select `RaftClusterStore` in `crates/node` when internodes enabled and remove interim LWW `CatalogDelta` shipper wiring from `004` once Raft path is live
- [ ] T024 [US1] Implement admin `Controllers` / `RaftStatus { group }` and CLI `spacestorage controllers` / `spacestorage raft-status` in `crates/node`, `crates/admin-proto`, `crates/spacestorage` showing cluster primary, secondaries, commit/term
- [ ] T025 [US1] Confirm data-path coordination remains on any member (voter or not) in `crates/node` / handler path — no redirect of tenant queries to cluster primary (`FR-008`); document in code comment at coordinator entry if needed

**Checkpoint**: Three-node election, failover, and minority refuse pass; `spacestorage controllers` reports one cluster primary. MVP gate for electable cluster metadata.

---

## Phase 4: User Story 2 - Namespace Raft and datatype leadership leases (Priority: P1)

**Goal**: One Raft group per namespace owning schemas, container definitions, shared-datatype metadata; leaderless KV writes never take a lease; ordered types get epoch-fenced leadership leases from the namespace group; namespace minorities isolated.

**Independent Test**: First binary: create namespace used by PostgreSQL/Redis (`016`), confirm its Raft group holds schemas and KV/table definitions, write to KV without a leadership lease (SC-005). Complete product: two namespaces + log-stream lease epoch fence (SC-006).

### Tests for User Story 2 ⚠️

- [ ] T026 [P] [US2] Add conformance test in `crates/conformance` that creating namespace `acme` starts `GroupId::Namespace` with primary+secondary and applied schemas/definitions; leaderless KV/SQL insert succeeds with no lease row (SC-005)
- [ ] T027 [P] [US2] Add unit tests in `crates/controlplane/src/lease.rs` for grant/steal epoch++ , `LeaseForbidden` on leaderless types, `StaleEpoch` refuse, and `LeaseNotGranted` when ordered write lacks a lease
- [ ] T028 [P] [US2] Add conformance test behind feature `controlplane-leases` (or ordered-type present) in `crates/conformance` for SC-006: re-grant lease → old holder stale-epoch appends refused and absent from ordered history

### Implementation for User Story 2

- [ ] T029 [P] [US2] Implement `NamespaceState` apply machine in `crates/controlplane/src/namespace.rs`: `schemas`, `containers` (definitions/options/shared-datatype metadata), `leases`, namespace `voter_set`, last applied `hlc` — MUST NOT hold cluster membership
- [ ] T030 [US2] On cluster commit of `create_namespace`, start namespace Raft group in `crates/controlplane` with initial voter set = current **cluster** voter set and all other members as learners; isolate majorities so loss of one namespace majority does not block another ([contracts/namespace-store.md](contracts/namespace-store.md))
- [ ] T031 [US2] Route schema/definition/shared-meta `ClusterStore::append` events to the owning namespace group in `crates/controlplane/src/apply.rs`; namespace minority → `Minority { group: ns/<id> }`
- [ ] T032 [US2] Implement `LeadershipLease { container_id, holder, epoch, granted_index }` grant/steal in `crates/controlplane/src/lease.rs` as namespace-log commits; leaderless types → `LeaseForbidden`; wall-clock alone MUST NOT fence ([contracts/leadership-lease.md](contracts/leadership-lease.md))
- [ ] T033 [US2] Enforce epoch on ordered append / `004` FR-078 path in `crates/controlplane` + consumer seam: require presented `epoch == current` else `StaleEpoch { have, need }` and MUST NOT enter ordered history; first-binary types MUST NOT take a lease (`FR-020`)
- [ ] T034 [US2] Stamp applied cluster/namespace records with HLC from `012` in `crates/controlplane/src/apply.rs` (`FR-015`) while Raft `(term, index)` remains commit order
- [ ] T035 [US2] Extend `spacestorage controllers` / admin `Controllers` in `crates/spacestorage` and `crates/admin-proto` to list each namespace group primary/secondaries; add `Leases { namespace }` (empty in first binary unless ordered type exists)

**Checkpoint**: Namespace Raft live for every existing namespace; leaderless writes unbound by leases; lease API ready for ordered types.

---

## Phase 5: User Story 3 - Restore on boot (Priority: P1)

**Goal**: On start, restore definitions/options always; persistent/hybrid content via `013`; memory-mode content empty unless replication re-populates; no vote / new replica target until membership records the node.

**Independent Test**: Create persistent, hybrid, and unreplicated memory-mode containers; restart the node; compare definitions vs content (SC-004). Process not in membership does not vote.

### Tests for User Story 3 ⚠️

- [ ] T036 [P] [US3] Add conformance restore suite in `crates/conformance` for SC-004: after restart, persistent/hybrid keep definition+content; unreplicated memory-mode keeps definition, empty content, volatility notice; replicated memory-mode may re-populate via `004` only
- [ ] T037 [P] [US3] Add unit/integration test in `crates/controlplane/src/restore.rs` (or conformance) that a process absent from membership starts without voting and refuses new replica placements (`NotMember` / `FR-010`)

### Implementation for User Story 3

- [ ] T038 [US3] Implement boot orchestrator `RestorePlan` in `crates/controlplane/src/restore.rs` ordered: open local Raft dirs → apply snapshots/logs to catalog → invoke `013` content restore for persistent/hybrid → memory content empty → internodes up → learner catch-up → optional `004` memory re-populate ([contracts/restore.md](contracts/restore.md))
- [ ] T039 [US3] Invoke `013` restore entrypoint from `crates/durability` inside `crates/controlplane/src/restore.rs` without reimplementing WAL; surface volatility notice from type/storage mode on unreplicated memory-mode
- [ ] T040 [US3] Handle corrupt/unknown Raft format in `crates/controlplane/src/raft_store.rs` / `restore.rs`: isolate group dir, attempt peer snapshot, else degrade; `UnknownRaftFormat` refuses start (`015`)
- [ ] T041 [US3] Hook restore before `ready` in `crates/node` so definitions are applied within the `001` ready bound once `013` replay finishes; ensure non-member join path (`011`) does not start as voter

**Checkpoint**: Restart preserves durable content and definitions; memory-mode volatility honored; non-members cannot vote.

---

## Phase 6: User Story 4 - Migrate a controller voter (Priority: P1; slice 7)

**Goal**: Operator migrates cluster/namespace voter without reconstructing the store; odd-sized majority preserved at every accepted step; default co-locate tenant data; `controller_exclusive_data` option (off by default) excludes voters as tenant replica targets and drains existing replicas.

**Independent Test**: Five-member cluster with voters A,B,C; migrate C→D; D votes, C does not, metadata survives (SC-009). Default placement MAY use voters; exclusive-data on excludes new tenant replicas (SC-010).

### Tests for User Story 4 ⚠️

- [ ] T042 [P] [US4] Add unit tests in `crates/controlplane/src/ops.rs` / `membership.rs` behind `controlplane-ops`: replace keeps odd size; grow/shrink by 2; step that loses majority or goes even → `VoterSetMajorityLost` / `VoterSetOdd` and previous set remains
- [ ] T043 [P] [US4] Add conformance tests behind feature `controlplane-ops` in `crates/conformance` for SC-009 voter replace and SC-010 exclusive-data placement exclusion / no in-place drop

### Implementation for User Story 4

- [ ] T044 [US4] Implement slice-7 membership ops in `crates/controlplane/src/ops.rs`: `Replace { from, to }`, `Grow { add: [NodeId; 2] }`, `Shrink { remove: [NodeId; 2] }` keeping odd size and majority at every accepted joint step (`FR-018`)
- [ ] T045 [US4] Implement live `controller_exclusive_data` flag in `crates/controlplane` / `crates/config`: default off; when on, if tenant replicas remain on voters → `ExclusiveDataBlocked` until `04`/`11` drain — never drop (`FR-019`)
- [ ] T046 [US4] Publish `tenant_replica_excluded(node) -> bool` from `crates/controlplane` and consume in `crates/placement` as `NodeFlags::no_tenant_data` / selector miss (`excluded: controller_exclusive_data`)
- [ ] T047 [US4] Add admin/CLI `VoterReplace` / `VoterGrow` / `VoterShrink` and `spacestorage controllers voters replace|grow|shrink` in `crates/admin-proto`, `crates/node`, `crates/spacestorage`; without `controlplane-ops` return `Slice7Required`
- [ ] T048 [US4] Gate `controlplane-ops` in `crates/release-profile` so first binary compiles cluster+namespace Raft with static voters and exclusive-data off; slice 7 enables ops feature

**Checkpoint**: Voter migration and exclusive-data work under `controlplane-ops`; first binary remains static/odd/off.

---

## Phase 7: User Story 5 - Secondaries for failover and read of metadata (Priority: P2)

**Goal**: Secondaries serve consistent metadata reads (read-index); metadata writes forward/redirect to primary; shared-datatype metrics aggregated in memory on the **namespace** primary only.

**Independent Test**: Issue describe/list against a secondary while primary is busy; stop primary; confirm failover (SC-007). Aggregates served only from namespace primary (SC-011).

### Tests for User Story 5 ⚠️

- [ ] T049 [P] [US5] Add conformance metadata-read tests in `crates/conformance` for SC-007: secondary list namespaces matches primary after read-index; write to secondary is forwarded or `NotLeader`, never applied only on secondary
- [ ] T050 [P] [US5] Add conformance aggregator tests in `crates/conformance` for SC-011: merged shared-datatype series on namespace primary `/metrics`; not on cluster primary (if different) as aggregator of record; local series always present

### Implementation for User Story 5

- [ ] T051 [US5] Implement follower metadata reads with read-index (or wait applied ≥ leader commit) in `crates/controlplane/src/read.rs`; learners with apply lag MUST wait or redirect rather than return stale-as-complete ([contracts/metadata-reads.md](contracts/metadata-reads.md))
- [ ] T052 [US5] Implement metadata write path on non-leader: `RaftForward` to leader if known else `NotLeader { leader }` in `crates/controlplane` / `crates/internode`; never apply write only on secondary
- [ ] T053 [US5] Implement in-memory `SharedMetricAggregate` merge on namespace primary in `crates/controlplane/src/metrics_agg.rs` via `MetricsPush` each `cluster.raft.heartbeat`; `stale=true` if replica silent for `2 * heartbeat`; cluster primary and lease holder MUST NOT aggregate (`FR-013`)
- [ ] T054 [US5] Expose merged series only on namespace-primary node exposition seam consumed by `08` in `crates/controlplane` / node metrics path; keep required `08` names unchanged; election series already labeled `raft_group`

**Checkpoint**: Secondary metadata reads match primary; namespace-primary aggregates shared-datatype metrics; failover still elects.

---

## Phase 8: Polish & Cross-Cutting Concerns

**Purpose**: Docs, profile alignment, quickstart proof, cleanup across stories

- [ ] T055 [P] Document `cluster.raft` and `controller_exclusive_data` starters in `docs/` (or extend `001`/`016` config docs) matching [contracts/config-directives.md](contracts/config-directives.md) production defaults and loopback fixtures
- [ ] T056 [P] Record `016` sequencing amendment note (Raft in first binary for cluster + existing namespaces; slice 7 keeps migrate/exclusive-data) in `specs/016-mvp-and-nongoals/` only if that feature’s contracts are opened for edit — otherwise leave a pointer comment in `crates/release-profile` feature docs
- [ ] T057 Run [quickstart.md](quickstart.md) validation end-to-end: config validate invalid fixtures; one-node controllers; three-node election/failover; minority refuse; restore split; secondary metadata read; aggregator identity
- [ ] T058 Code cleanup in `crates/controlplane`: ensure no fsync on Tokio workers, no etcd/gRPC, no per-container Raft, and leaderless write path has zero extra RPC to a datatype primary
- [ ] T059 [P] Confirm double-bootstrap same cluster name is detected/not merged via `011` seam (store refuses second UUID) in `crates/controlplane/src/cluster.rs` / membership integration

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Setup — **BLOCKS** all user stories
- **User Story 1 (Phase 3)**: After Foundational — 🎯 MVP (cluster Raft)
- **User Story 2 (Phase 4)**: After Foundational; practically needs US1 cluster commit path for `create_namespace` (ties with US1 per spec)
- **User Story 3 (Phase 5)**: After Foundational; needs US1/US2 applied state to restore definitions meaningfully
- **User Story 4 (Phase 6)**: After Foundational + US1 voter model; slice 7 / `controlplane-ops` — can proceed in parallel with US5 once US1 exists
- **User Story 5 (Phase 7)**: After Foundational + US1/US2 groups electing; builds on secondary roles
- **Polish (Phase 8)**: After desired stories complete

### User Story Dependencies

- **US1 (P1)**: No dependency on other stories — electable cluster metadata MVP
- **US2 (P1)**: Uses cluster commit to create namespaces; independently testable once a namespace exists
- **US3 (P1)**: Independently testable restore split; full value after US1/US2 state machines exist
- **US4 (P1 / slice 7)**: Extends voter set ops; first binary MAY skip
- **US5 (P2)**: Extends read/forward/metrics on existing groups; independently testable for metadata-read SC-007

### Within Each User Story

- Tests (where listed) MUST be written and FAIL before implementation
- Models/state machines before Raft apply routing
- Services/apply before admin/CLI
- Story complete before moving to next priority when staffing sequentially

### Parallel Opportunities

- Phase 1: T004 alongside T001–T003
- Phase 2: T006, T007, T009, T010, T011, T013 in parallel after T005
- US1 tests T014–T017 in parallel; then implementation
- US2 tests T026–T028 in parallel; `namespace.rs` (T029) parallel with early lease unit work (T027)
- US4 and US5 can proceed in parallel after US1/US2 foundations once staffed
- Polish doc tasks T055–T056 parallel

---

## Parallel Example: User Story 1

```bash
# Launch US1 tests together:
Task: "Unit tests for first-binary voter table in crates/controlplane/src/membership.rs"
Task: "Conformance election SC-001/002/008 in crates/conformance"
Task: "Conformance minority SC-003 in crates/conformance"
Task: "Config invalid fixture tests in crates/config"

# After tests fail, launch independent implementation files:
Task: "ClusterState machine in crates/controlplane/src/cluster.rs"
Task: "Admin ControllerView stubs already in admin-proto from Phase 2"
```

---

## Parallel Example: User Story 2

```bash
# Launch US2 tests together:
Task: "Conformance namespace Raft + leaderless KV in crates/conformance"
Task: "Lease epoch unit tests in crates/controlplane/src/lease.rs"
Task: "SC-006 stale-epoch conformance behind controlplane-leases"

# Launch models in parallel:
Task: "NamespaceState in crates/controlplane/src/namespace.rs"
Task: "LeadershipLease API in crates/controlplane/src/lease.rs"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL)
3. Complete Phase 3: User Story 1 (cluster Raft election, minority refuse, metrics)
4. **STOP and VALIDATE**: Independent Test for US1 / SC-001–003, SC-008
5. Demo electable cluster metadata

### First-binary ship (FR-020)

Continue through **US2** (namespace Raft, no required leases) and **US3** (restore) before calling slices 1–5 done. Skip US4 (`controlplane-ops`) and optional SC-006 until ordered types / slice 7.

### Incremental Delivery

1. Setup + Foundational → foundation ready
2. US1 → MVP cluster control plane
3. US2 → per-namespace metadata CP + lease API
4. US3 → constitution restore rule
5. US5 → secondary reads + namespace metrics aggregate
6. US4 → slice 7 voter migrate + exclusive-data
7. Each story adds value without breaking prior stories

### Parallel Team Strategy

1. Team completes Setup + Foundational together
2. Dev A: US1 → US3 restore hook
3. Dev B: US2 namespace + leases
4. Dev C: US5 reads/metrics (after groups elect)
5. Dev D (later): US4 `controlplane-ops`

---

## Notes

- [P] = different files, no dependencies on incomplete tasks
- [Story] labels map to spec user stories US1–US5
- First binary MUST NOT require Log Stream or voter migration
- Raft RPCs only on `internode`; fsync off Tokio workers
- Commit after each task or logical group
- Stop at any checkpoint to validate the story independently
- Avoid: per-container Raft, LWW merge of ordered histories, aggregating shared metrics on cluster primary or lease holder
