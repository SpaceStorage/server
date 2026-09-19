---
description: "Task list for distribution, placement, media, and replication (L1)"
---

# Tasks: Distribution, Placement, Media, and Replication (L1 Shared Capabilities)

**Input**: Design documents from `/specs/004-distribution-placement/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md), [data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: Requested. Plan Testing section + SC-001–SC-018 + Independent Tests in [spec.md](spec.md) + contract fixtures under `contracts/fixtures/`. Include unit, contract, and multi-node conformance tasks. Write failing tests first within each story.

**Scope of this feature**: Grow `crates/placement` from the `003` seam into the L1 engine; add `crates/internode`; wire config/node/admin-proto/spacestorage/exec/conformance. Do **not** implement Raft (`06`), join/leave identity (`11`), full `quorum_domain`/HLC product surface (`12`), WAL ack meaning (`13`), query IR (`05`), or metric series naming (`08`). Consume seams those features own.

**Sibling crates** (from plan): `crates/placement`, `crates/internode` (new), `crates/config`, `crates/node`, `crates/admin-proto`, `crates/spacestorage`, `crates/exec`, `crates/conformance`.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: [US1]…[US8] on user-story phase tasks only
- Every task includes an exact file path

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Workspace member, placement module skeleton, docs/examples trees, starter fixture copies

- [X] T001 Create `crates/internode/Cargo.toml` (package `spacestorage-internode`, edition 2024) and `crates/internode/src/lib.rs` that `mod`s `frame`, `rpc`, `heartbeat`, `auth`
- [ ] T002 Add `crates/internode` to workspace `[workspace.members]` in `Cargo.toml` and depend from `crates/node` / `crates/spacestoraged` without pulling TCP into `crates/placement`
- [ ] T003 Extend `crates/placement/src/lib.rs` to declare modules `topology`, `selector`, `planner`, `replica`, `quorum`, `stamp`, `group`, `shard`, `repair`, `rebalance`, `txn`, `error`, `director` while keeping `matrix.rs` and `local.rs` compiling for `003`
- [ ] T004 [P] Create `docs/placement.md` and `docs/replication.md` stubs that link to [contracts/topology.md](contracts/topology.md) and [contracts/quorum.md](contracts/quorum.md)
- [ ] T005 [P] Copy the six FR-075 starter fixtures from `specs/004-distribution-placement/contracts/fixtures/` into `docs/examples/` (`node-single.conf`, `node-rack.conf`, `cluster-3az/`, `cluster-mixed-media/`, `cluster-two-region/`)

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Error codes, ClusterStore seam, additive director defaults, HLC stamps, internodes frame/auth skeleton, multi-node conformance harness. No user-story work until this phase is complete.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [ ] T006 Implement `PlacementError` and validation codes in `crates/placement/src/error.rs` per [data-model.md](data-model.md) (`PlacementUnsatisfiable`, `AntiAffinityKeyMissing`, `InternodesRequired`, `QuorumUnsatisfiable`, `QuorumRequiresAsyncGroup`, `QuorumNotDurable`, `EachQuorumRefused`, `ShardKeyUnknown`, `DecommissionBlocked`, `SelectorSyntax`, `DriveMediaRequired`, `MemorySizeRequired`, `NodeNameConflict`, `PeerUnknown`) with FR-082 refusal fields (container, constraint/level, topology facts, resolution hint)
- [ ] T007 [P] Implement `VersionStamp` HLC `(physical_micros, logical, node_id)` with total order and LWW compare in `crates/placement/src/stamp.rs` (skew gauge hook; default `max_stamp_skew` 500 ms configurable later)
- [ ] T008 [P] Define `ClusterStore` trait + append-only `PlacementEvent` log/snapshot types in `crates/placement/src/cluster_store.rs` (or `store.rs`) covering NodeJoin/Leave, Label/Drive/MemoryChange, PlacementPut/Delete, ReplicaHealth, RebalanceStep, RepairProgress, Txn* per [data-model.md](data-model.md) §17; LWW by event `id` + HLC until `06` fronts Raft
- [ ] T009 Grow `PlacementDirector` in `crates/placement/src/lib.rs` / `director.rs` with **default-bodied** additive methods only so `003` type-system tests keep compiling; keep `LocalDirector` in `crates/placement/src/local.rs` as RF=1 default when internodes is disabled
- [ ] T010 Implement length-prefixed frame codec in `crates/internode/src/frame.rs` matching `001` admin envelope style (no gRPC)
- [ ] T011 [P] Implement shared-token auth + optional TLS path wiring in `crates/internode/src/auth.rs` (token file path reference, mode ≤ 0600, never inlined; cert paths reuse `001` rules)
- [ ] T012 Stub `InternodeHandler` + `PeerSet` in `crates/internode/src/lib.rs` registering as a `001` `Handler` on its own port (explicitly enabled like `admin`)
- [ ] T013 Add multi-node in-process harness helpers in `crates/conformance/src/cluster_harness.rs` (or `tests/common/mod.rs`) that boot N nodes with ephemeral internodes/admin ports, shared fixtures under `contracts/fixtures/`, and optional injected RTT delay on the internodes path
- [ ] T014 [P] Add unit tests for `VersionStamp` ordering and `PlacementError` FR-082 shape in `crates/placement/src/stamp.rs` / `crates/placement/src/error.rs`

**Checkpoint**: `cargo test -p spacestorage-placement` and `cargo test -p spacestorage-internode` compile; `003` still green against additive director defaults. User stories can start.

---

## Phase 3: User Story 1 - Describe the cluster: nodes, labels, drives, and memory (Priority: P1) 🎯 MVP

**Goal**: Cluster-declared topology ladder; nodes fill every ladder key; drives and optional memory pools; hierarchy integrity; queryable topology view and placement domains from any node (FR-001–FR-010).

**Independent Test**: Declare labels/drives/memory on one or more nodes, read topology view, mutate labels/drives live, submit invalid declarations; no user data placement required.

### Tests for User Story 1 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T015 [P] [US1] Contract-validate all valid/invalid topology fixtures under `specs/004-distribution-placement/contracts/fixtures/` in `crates/conformance/tests/topology_fixtures.rs` (invalid filenames name expected codes)
- [ ] T016 [P] [US1] Add SC-001/SC-002 conformance in `crates/conformance/tests/topology_view.rs`: three-AZ fixture topology identical on every node; domains cardinalities; starter examples match stated domains
- [ ] T017 [P] [US1] Add unit tests for ladder fill, hierarchy integrity (`az` shared ⇒ coarser keys must match), and domain cardinality in `crates/placement/src/topology.rs`

### Implementation for User Story 1

- [ ] T018 [P] [US1] Implement `Node`, `Drive`, `MemoryPool`, `PlacementDomain`, `TopologyView`, and `TopologyLadder` (ordered subsequence of reserved keys `rack`/`az`/`region`/`continent`/`planet` + optional custom append; `planet` never a voting domain) in `crates/placement/src/topology.rs` with invariants from [data-model.md](data-model.md) §§1–4
- [ ] T019 [P] [US1] Implement `LabelSelector` parse/match AST in `crates/placement/src/selector.rs` per [contracts/label-selector.md](contracts/label-selector.md) (equality, inequality, set, presence, and/or, `media=` / `memory` sugar; no regex)
- [ ] T020 [US1] Parse config blocks `labels {}`, `storage.drive {}`, `memory { size; labels }`, `cluster { peers; topology_ladder; }` in `crates/config` (files under `crates/config/src/`) per [contracts/config-directives.md](contracts/config-directives.md) and R2/R16; refuse `DriveMediaRequired`, `MemorySizeRequired`, `NodeNameConflict`, duplicate keys without partial apply (FR-008)
- [ ] T021 [US1] Enforce ladder fill and hierarchy integrity on join/ready and live label changes in `crates/node` (omit ladder key ⇒ not `ready` as replica target; hierarchy break refused)
- [ ] T022 [US1] Persist and replicate topology via `ClusterStore` + internodes `CatalogDelta` / `Heartbeat` in `crates/internode/src/rpc.rs` and `crates/internode/src/heartbeat.rs` so every node resolves labels identically (FR-002, FR-006)
- [ ] T023 [US1] Expose `TopologyView` DTOs in `crates/admin-proto` and `topology` admin/CLI commands in `crates/spacestorage` (domains per key, derived vs declared labels, drive free space, memory usage)
- [ ] T024 [US1] Apply live label/drive/memory mutations without restart (except `node.name` / internodes bind) and emit `PlacementEvent::Topology` in `crates/placement` / `crates/node` (FR-009)

**Checkpoint**: Three-AZ loopback cluster describes identical topology; invalid fixtures refuse by code; US1 Independent Test passes.

---

## Phase 4: User Story 2 - Replicate a container with a factor and anti-affinity by label (Priority: P1)

**Goal**: Deterministic placement planner, replica sets, strict anti-affinity (default finest ladder key), inspectable reports, `ClusterDirector` when internodes enabled (FR-011–FR-027).

**Independent Test**: Create containers at RF 1/2/3 under anti-affinity keys; describe placement; refuse unsatisfiable pairs; restart a node; same answer from every node.

### Tests for User Story 2 ⚠️

- [ ] T025 [P] [US2] Add SC-003 conformance in `crates/conformance/tests/anti_affinity.rs` (satisfiable spreads + Q5 refusals with required/available counts; no storage allocated on refuse)
- [ ] T026 [P] [US2] Add planner unit tests in `crates/placement/src/planner.rs` for deterministic tie-break on `node_name`, ordered anti-affinity lists, and exclusion reasons

### Implementation for User Story 2

- [ ] T027 [P] [US2] Implement `Replica`, `ReplicaSet`, health state machine (`empty`/`in_sync`/`behind`/`unavailable`/`moving`/`removed`/`content_lost`) in `crates/placement/src/replica.rs` per [data-model.md](data-model.md) §9–10
- [ ] T028 [US2] Implement deterministic greedy planner in `crates/placement/src/planner.rs` (selector exclude → media/memory capacity → order by anti-affinity distinctness, least load, `node_name`; refuse unsatisfiable — never silent downgrade) per R4 and [contracts/placement.md](contracts/placement.md)
- [ ] T029 [US2] Record `Placement` / `PlacementReport` (targets, exclusions, satisfied/degraded/unplaceable) in `ClusterStore` and implement `ClusterDirector` in `crates/placement/src/director.rs` as default when internodes is enabled; RF>1 without internodes ⇒ `InternodesRequired`
- [ ] T030 [US2] Default anti-affinity for RF≥2 to finest ladder key; support ordered key lists and per-label-value replica counts (FR-020, FR-023) in `crates/placement/src/planner.rs` / constraint types
- [ ] T031 [US2] Wire `003` capability declarations into `ClusterDirector` so replication/node_placement/labels/persistent_placement execute or refuse with named reason; RF change places/removes replicas under constraints (FR-025) in `crates/placement` + type-system call sites
- [ ] T032 [US2] Expose placement/replica-set describe in `crates/admin-proto` and `crates/spacestorage` CLI (`placement`, `replicas`) readable from any node (FR-018, FR-024, FR-080)
- [ ] T033 [US2] On after-the-fact anti-affinity violation (label change / node loss), mark placement `degraded` with named constraint and keep serving (FR-026) in `crates/placement/src/director.rs` / `crates/placement/src/replica.rs`; repair deferred to US8 rebalance hook

**Checkpoint**: Anti-affinity placements and refusals match SC-003; every node returns the same replica map.

---

## Phase 5: User Story 3 - Pick a quorum per query, from any node (Priority: P1)

**Goal**: Cassandra quorum vocabulary + defaults (write `TWO`, read `ONE`), durable-ack filter (Q3), leaderless any-node coordination via internodes fan-out, live `PlacementInfo` (FR-028–FR-037, FR-045–FR-049).

**Independent Test**: Three-node cluster; write/read each level from replica and non-replica nodes; omit level for defaults; stop replicas; inspect applied options / execution record.

### Tests for User Story 3 ⚠️

- [ ] T034 [P] [US3] Add SC-005/SC-006 conformance in `crates/conformance/tests/quorum_levels.rs` (vocabulary, defaults, explicit reject vs default clamp, durable vs memory acks Q3)
- [ ] T035 [P] [US3] Add SC-007 conformance in `crates/conformance/tests/coordinator_any_node.rs` (identical results from replica vs non-replica coordinator; coordinator named in execution record)
- [ ] T036 [P] [US3] Add quorum arithmetic unit tests in `crates/placement/src/quorum.rs` for factors 2–5 and `Acks(n)`

### Implementation for User Story 3

- [ ] T037 [P] [US3] Implement quorum level arithmetic and durable-ack filter in `crates/placement/src/quorum.rs` per [contracts/quorum.md](contracts/quorum.md) (persistent/hybrid count drive-backed only; memory-mode counts memory; mixed report `ack_kind`)
- [ ] T038 [US3] Implement precedence query → session → container → namespace → global and clamp/reject rules in `crates/placement/src/quorum.rs` + `crates/exec` option resolution (FR-029–FR-033); extend `ExecutionRecord.applied` with `QuorumDecision` fields (FR-035)
- [ ] T039 [US3] Implement `FanoutWrite` / `FanoutRead` RPC and coordinator fan-out in `crates/internode/src/rpc.rs` and `crates/exec` `Coordinator` so any member node coordinates; replicas accept writes independently (leaderless; Clarification Q1)
- [ ] T040 [US3] Implement live `PlacementInfo` against replica sets (not stub `replicas()==1`) in `crates/exec` / `crates/placement`; keep single-node `LocalDirector` path for RF=1 clamping tests
- [ ] T041 [US3] Apply LWW by `VersionStamp` on read disagreement and record correction intent in `crates/placement/src/stamp.rs` + coordinator path (FR-037); type-supported alternate merge hook from type descriptor
- [ ] T042 [US3] Refuse rather than guess when placement view is known stale; record coordinating node and contacted replicas (FR-047–FR-048) in `crates/exec`

**Checkpoint**: SC-005–SC-007 green on three-node harness; defaults and durable-ack rules hold.

---

## Phase 6: User Story 4 - Put each datatype on the right media (Priority: P2)

**Goal**: Media and memory constraints select drives/pools; capacity exclusions; memory restart repopulate semantics (FR-003–FR-005, FR-010, media portions of FR-012–FR-015).

**Independent Test**: Mixed-media cluster; place NVMe/HDD/memory containers; fill capacity; refuse absent media; inspect drive/pool usage.

### Tests for User Story 4 ⚠️

- [ ] T043 [P] [US4] Add SC-004 conformance in `crates/conformance/tests/media_constraints.rs` (matching media only; unplaceable with exclusion reasons; memory-only nodes excluded when no pool)
- [ ] T044 [P] [US4] Add unit tests for media sugar and capacity exclusion reasons in `crates/placement/src/planner.rs` / `selector.rs`

### Implementation for User Story 4

- [ ] T045 [US4] Enrich node derived labels from drive media kinds and pin replicas to specific matching drives or memory pools in `crates/placement/src/topology.rs` and `crates/placement/src/planner.rs` (FR-004, FR-012)
- [ ] T046 [US4] Exclude nodes with full matching drives or insufficient memory room with reasons `no_media_capacity` / `no_memory_pool` in `crates/placement/src/planner.rs`; never place on wrong media (FR-010, FR-013)
- [ ] T047 [US4] On memory-mode node restart, mark local replica `empty`→`behind` while repopulating from a live replica; report `content_lost` only when no live replica remains in `crates/placement/src/replica.rs`
- [ ] T048 [US4] Report per-drive and per-memory-pool capacity, used space, and resident containers via admin/CLI in `crates/spacestorage` / `crates/admin-proto`
- [ ] T049 [US4] Combine media + locality + anti-affinity constraints so all hold for every replica; refuse naming the unsatisfied constraint (FR-015) in `crates/placement/src/planner.rs`

**Checkpoint**: SC-004 green; mixed-media fixture placements match contracts.

---

## Phase 7: User Story 5 - Keep serving through a node, rack, or AZ failure (Priority: P2)

**Goal**: Failure detection, hinted handoff, replay vs full-compare repair, partition/heal with LWW, decommission re-place (FR-057–FR-064).

**Independent Test**: Stop one replica, then majority, then a rack; operate at each quorum; restart and verify catch-up; partition/heal with conflict report.

### Tests for User Story 5 ⚠️

- [ ] T050 [P] [US5] Add SC-008 conformance in `crates/conformance/tests/replica_down.rs` (ONE/TWO/QUORUM succeed with one down; ALL fails; catch-up loses 0 acknowledged writes)
- [ ] T051 [P] [US5] Add SC-009 conformance in `crates/conformance/tests/partition_heal.rs` (no ack on side below quorum; heal converges by LWW; conflicts reported)
- [ ] T052 [P] [US5] Add unit tests for hint expiry window (default 3 h per R10) and repair job state machine in `crates/placement/src/repair.rs`

### Implementation for User Story 5

- [ ] T053 [US5] Drive replica `unavailable` from internodes heartbeat timeout (fixed interval, not phi-accrual per R18) in `crates/internode/src/heartbeat.rs` and `crates/placement/src/replica.rs`; mark placements degraded (FR-057–FR-058)
- [ ] T054 [US5] Implement hinted handoff store under `catalog/hints/` and `Hint` RPC in `crates/placement/src/repair.rs` + `crates/internode/src/rpc.rs`; replay on return within window (FR-059)
- [ ] T055 [US5] Implement full-compare `RepairJob` in `crates/placement/src/repair.rs` staging under `catalog/repair/` when window exceeded; throttleable background compare (FR-060, FR-062)
- [ ] T056 [US5] On read disagreement, return LWW (or type merge) to client and schedule stale correction (FR-061) in coordinator path `crates/exec`
- [ ] T057 [US5] During partition, refuse writes that cannot reach required acks; on heal converge and report resolved conflicts (FR-063) in `crates/placement` / `crates/exec`
- [ ] T058 [US5] Permanent node removal re-places replicas under constraints with visible progress or `DecommissionBlocked{containers}` (FR-064) in `crates/placement/src/rebalance.rs` seam (full planner in US8)
- [ ] T059 [US5] List every degraded placement with container, constraint at risk, failure, and repair in progress via admin health in `crates/spacestorage` / `crates/admin-proto` (FR-080)

**Checkpoint**: SC-008/SC-009 green; hints and compare repair observable.

---

## Phase 8: User Story 6 - Shard and partition a container that outgrows one node (Priority: P2)

**Goal**: Hash sharding (rendezvous), range/time partitions, any-node routing, multi-shard fan-out with per-shard quorum, split without loss (FR-050–FR-056).

**Independent Test**: Create sharded and partitioned containers; write keys across shards; multi-key query from non-owner node; add a shard; refuse invalid keys.

### Tests for User Story 6 ⚠️

- [ ] T060 [P] [US6] Add SC-011/SC-012 conformance in `crates/conformance/tests/shards.rs` (bijection during split; multi-shard query fails naming shard unless partial requested)
- [ ] T061 [P] [US6] Add unit tests for rendezvous bijection and partition bucket routing in `crates/placement/src/shard.rs`

### Implementation for User Story 6

- [ ] T062 [P] [US6] Implement `ShardMap` (hash + rendezvous) and `PartitionMap` (range|time) in `crates/placement/src/shard.rs` per [contracts/sharding-partitioning.md](contracts/sharding-partitioning.md) and R11/R12
- [ ] T063 [US6] Place each shard/partition as its own `ReplicaSet` under container RF/anti-affinity/media/locality in `crates/placement/src/planner.rs` / `director.rs`
- [ ] T064 [US6] Route keys from any coordinator to exactly one shard+partition; fan-out multi-shard queries with per-shard quorum and explicit partial-failure (FR-052–FR-053) in `crates/exec` Coordinator
- [ ] T065 [US6] Support background shard-count / boundary changes with continuous key resolvability and no ack loss/duplication (FR-054) in `crates/placement/src/shard.rs`
- [ ] T066 [US6] Refuse unsupported/conflicting/missing shard keys with `ShardKeyUnknown` / named conflict and nothing placed (FR-055) in `crates/placement` + type-system handoff
- [ ] T067 [US6] Expose per-shard volume/request-rate and imbalance threshold as rebalance candidates in `crates/node/src` stats hooks / admin (FR-056)

**Checkpoint**: SC-011/SC-012 green; shard describe lists ranges and replicas.

---

## Phase 9: User Story 7 - Keep data near the user and replicate slowly to remote regions (Priority: P3)

**Goal**: Destination groups (sync local / async remote), lag reporting, local writes skip remote RTT, `EACH_QUORUM` refuse-by-default (Q4), planetary-tolerant timeouts (FR-038–FR-044, FR-071–FR-075).

**Independent Test**: Two-region fixture with injected delay; local write latency excludes remote RTT; inspect lag; cut/heal link; `EACH_QUORUM` refused unless wait opted in.

### Tests for User Story 7 ⚠️

- [ ] T068 [P] [US7] Add SC-010 conformance in `crates/conformance/tests/local_async_remote.rs` (source-domain write excludes follower RTT; lag continuous; Q4 `EACH_QUORUM` refuse vs wait)
- [ ] T069 [P] [US7] Add unit tests for `each_quorum_policy` default `refuse` and group lag health in `crates/placement/src/group.rs`

### Implementation for User Story 7

- [ ] T070 [P] [US7] Implement `DestinationGroup` (selector, factor, sync|async, lag threshold, `each_quorum_policy`) in `crates/placement/src/group.rs` per [contracts/replication.md](contracts/replication.md)
- [ ] T071 [US7] Rank candidates by first differing ladder key; override with measured internodes RTT once samples exist (FR-001b) in `crates/placement/src/planner.rs` / peer RTT from `crates/internode`
- [ ] T072 [US7] Ensure local/source write levels do not wait on async remote groups; async stream exposes backlog/lag/health (FR-041, FR-072) in coordinator + `crates/placement/src/group.rs`
- [ ] T073 [US7] Refuse `EACH_QUORUM` / levels needing async-group ack unless container opts into wait; never treat queued send as ack (FR-074, Q4) in `crates/placement/src/quorum.rs`
- [ ] T074 [US7] Make failure-detection, lag, repair, and retry budgets configurable without assuming low RTT; document minutes-scale example in `docs/replication.md` (FR-073, FR-075)
- [ ] T075 [US7] Validate two-region starter fixture end-to-end against stated modes/quorum in `crates/conformance/tests/local_async_remote.rs` / topology fixtures (SC-002 subset)

**Checkpoint**: SC-010 green with injected delay; two-region starter matches docs.

---

## Phase 10: User Story 8 - Rebalance after the cluster changes (Priority: P3)

**Goal**: Constraint-safe rebalance plans on add/remove/relabel/RF/constraint change; throttle, pause/resume, resume-after-failure (FR-065–FR-070).

**Independent Test**: Add node, decommission, relabel; inspect plan/progress/throttle; pause/resume; verify constraints and integrity.

### Tests for User Story 8 ⚠️

- [ ] T076 [P] [US8] Add SC-013/SC-014 conformance in `crates/conformance/tests/rebalance_lifecycle.rs` (add/decommission/relabel; resume after interrupt; 0 residual copies; quorum availability held)
- [ ] T077 [P] [US8] Add unit tests for plan step ordering (anti-affinity invariant or declared temporary violation) in `crates/placement/src/rebalance.rs`

### Implementation for User Story 8

- [ ] T078 [US8] Implement `RebalancePlan` / `Move` / throttle / pause / resume token in `crates/placement/src/rebalance.rs` per [contracts/rebalancing.md](contracts/rebalancing.md) and R13
- [ ] T079 [US8] Generate plans on node add/remove, label change, RF change, and constraint change in `crates/placement/src/rebalance.rs`; respect constraints each step or declare bounded temporary violations (FR-065–FR-066)
- [ ] T080 [US8] Run moves in background via internodes `Rebalance` RPC while serving at declared quorum; operator-adjustable `rate_bytes_per_sec` (FR-067) in `crates/internode/src/rpc.rs` + `crates/placement/src/rebalance.rs`
- [ ] T081 [US8] Persist plan cursor via `ClusterStore` in `crates/placement/src/rebalance.rs` / `crates/placement/src/cluster_store.rs`; resume after node/controller failure without loss/duplication (FR-068–FR-069)
- [ ] T082 [US8] Block decommission with `DecommissionBlocked{containers}` in `crates/placement/src/rebalance.rs` / `crates/placement/src/error.rs` when no target satisfies constraints (FR-064/FR-070)
- [ ] T083 [US8] Expose plan inspect (moved/in-flight/remaining/rate/ETA) via `crates/admin-proto` and `crates/spacestorage` CLI

**Checkpoint**: SC-013/SC-014 green; pause/resume and blocked decommission verified.

---

## Phase 11: Polish & Cross-Cutting Concerns

**Purpose**: 2PC (FR-076–FR-079), observability counters (FR-081), refusal sweep (SC-016), capability matrix (SC-018), docs and quickstart validation

### Tests (cross-cutting) ⚠️

- [ ] T084 [P] Add SC-015 conformance in `crates/conformance/tests/two_phase_commit.rs` (participant killed mid-commit → single durable outcome; no indefinite `in_doubt` past bound)
- [ ] T085 [P] Add SC-016 refusal-shape conformance in `crates/conformance/tests/refusal_shape.rs`
- [ ] T086 [P] Add SC-017 counters conformance in `crates/conformance/tests/placement_stats.rs`
- [ ] T087 [P] Add SC-018 capabilities-executed conformance in `crates/conformance/tests/capabilities_executed.rs` (all 13 L1 capabilities executed or refused, never ignored)
- [ ] T088 [P] Add contract tests that every fixture under `specs/004-distribution-placement/contracts/fixtures/` validates in `crates/conformance/tests/topology_fixtures.rs` (extend T015 coverage for replication/cluster invalids)

### Implementation

- [ ] T089 Implement 2PC coordinator/participant state machine and durable log records in `crates/placement/src/txn.rs` per [contracts/distributed-transactions.md](contracts/distributed-transactions.md); internodes `Txn*` RPCs in `crates/internode/src/rpc.rs`
- [ ] T090 Consume control-plane leadership seam for single-writer/ordered types only (FR-078–FR-079) without reimplementing Raft; keep ordinary containers leaderless in `crates/placement/src/director.rs`
- [ ] T091 Emit FR-081 counters (acks required/achieved, quorum refusals, lag, repair volume, rebalance volume) in `crates/node` stats with additive labels only (`container`, `capability`, `quorum`, `ack_kind`, locality) — no `08` renames
- [ ] T092 [P] Complete `docs/placement.md`, `docs/replication.md`, and FR-075 starter narrative so quickstart examples work verbatim
- [ ] T093 Run [quickstart.md](quickstart.md) validation end-to-end on loopback (topology → anti-affinity → quorum → media → failure → shards → two-region → rebalance) and fix gaps
- [ ] T094 [P] Confirm `LocalDirector` in `crates/placement/src/local.rs` and `003` CI still pass against additive `PlacementDirector` defaults in `crates/placement/src/lib.rs` after all methods land

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Setup — **BLOCKS** all user stories
- **US1 (Phase 3)**: After Foundational — MVP topology
- **US2 (Phase 4)**: After US1 (planner needs topology/selector)
- **US3 (Phase 5)**: After US2 (quorum needs replica sets + internodes fan-out)
- **US4 (Phase 6)**: After US2 (media extends planner); can overlap late US3
- **US5 (Phase 7)**: After US3 (failure/repair needs quorum + fan-out)
- **US6 (Phase 8)**: After US2–US3 (shards are replica sets + coordinator routing)
- **US7 (Phase 9)**: After US3 (groups/lag/`EACH_QUORUM`); benefits from US5 for link cut/heal
- **US8 (Phase 10)**: After US2 and ideally US5–US6 (rebalance moves replicas/shards)
- **Polish (Phase 11)**: After desired stories; 2PC after US3 coordinator exists

### User Story Dependencies

- **US1 (P1)**: After Foundational only — MVP
- **US2 (P1)**: Needs US1 topology/selector
- **US3 (P1)**: Needs US2 replica sets
- **US4 (P2)**: Needs US2 planner; independent of US3 for placement-only tests
- **US5 (P2)**: Needs US3 quorum/coordinator
- **US6 (P2)**: Needs US2+US3; independent of US7
- **US7 (P3)**: Needs US3; composes US1 labels + groups
- **US8 (P3)**: Needs US2; integrates US5 decommission and US6 shard moves

### Within Each User Story

- Tests MUST be written and FAIL before implementation
- Models/types before services/planner
- Planner/replica before coordinator endpoints
- Story complete before relying on it from a later story

### Parallel Opportunities

- Phase 1: T004/T005 parallel; T001–T003 sequential with workspace wiring
- Phase 2: T007/T008/T011/T014 parallel after T006 codes exist
- US1: T015–T017 tests parallel; T018/T019 parallel
- US2–US8: each story’s `[P]` test tasks parallel; cross-story parallel only after shared deps land (e.g. US4 || late US3; US6 || US7 after US3)
- Polish: T084–T088 and T092/T094 parallel

---

## Parallel Example: User Story 1

```bash
# Tests first (parallel):
Task: "T015 Contract-validate topology fixtures in crates/conformance/tests/topology_fixtures.rs"
Task: "T016 SC-001/SC-002 topology_view.rs"
Task: "T017 Unit tests in crates/placement/src/topology.rs"

# Models (parallel):
Task: "T018 Topology types in crates/placement/src/topology.rs"
Task: "T019 LabelSelector in crates/placement/src/selector.rs"
```

## Parallel Example: User Story 3

```bash
Task: "T034 quorum_levels.rs SC-005/006"
Task: "T035 coordinator_any_node.rs SC-007"
Task: "T036 quorum arithmetic unit tests in crates/placement/src/quorum.rs"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL)
3. Complete Phase 3: User Story 1 (topology view + ladder + fixtures)
4. **STOP and VALIDATE**: `cargo test -p spacestorage-conformance` topology tests + [quickstart.md](quickstart.md) §1
5. Demo three-AZ `spacestorage topology` from any admin port

### Incremental Delivery

1. Setup + Foundational → seams compile
2. US1 topology → demo cluster describe
3. US2 anti-affinity placement → SC-003
4. US3 quorum + any-node coordinator → SC-005–007
5. US4 media → SC-004
6. US5 failure/repair → SC-008–009
7. US6 shards → SC-011–012
8. US7 multi-region async → SC-010
9. US8 rebalance → SC-013–014
10. Polish 2PC + SC-015–018 + docs

### Parallel Team Strategy

1. Team completes Setup + Foundational together
2. After Foundational:
   - Developer A: US1 → US2 → US4
   - Developer B: internodes RPC + US3 → US5
   - Developer C: US6 → US7 → US8 (after US2/US3 checkpoints)
3. Everyone converges on Polish (2PC, SC-018, docs)

---

## Notes

- [P] = different files, no dependencies on incomplete tasks
- [US1]…[US8] map to spec stories (P1: US1–US3; P2: US4–US6; P3: US7–US8)
- Suggested plan waves (1)–(9) align to phases 3–11
- Do not invent Raft, gRPC, or a second client protocol
- `PlacementDirector` growth must stay additive with default bodies
- Verify tests fail before implementing
- Commit after each task or logical group
- Stop at any checkpoint to validate a story independently
