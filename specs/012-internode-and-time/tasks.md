---
description: "Task list for internode fabric, clocks, quorum domain, and conflict resolution"
---

# Tasks: Internode Fabric, Clocks, Quorum Domain, and Conflict Resolution

**Input**: Design documents from `/specs/012-internode-and-time/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md), [data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: Requested. Plan Testing section + Independent Tests + SC-001–SC-009 + in-process conformance/`contracts/fixtures/`. Unit tests in `clocks`, `internode`, `replication`; contract/fixture validation; `crates/conformance` harness. Write failing tests first where listed under a story.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

**Scope of this feature**: Own `crates/internode`; add `crates/clocks` and `crates/replication`; wire always-on handlers, `quorum_domain`, source vs log-follower, HLC in-domain, cluster-wide FD, promote/fence, `multi_active` refuse. Placement RF/repair **policy** stays `004`; join/bootstrap stay `011`; durable WAL meaning stays `013`; Raft RPCs stay on `internode` (`006`).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: [US1], [US2], [US3], [US4] on user-story phase tasks only
- Every task includes an exact file path

## Path Conventions

- Workspace crates under `crates/` at repository root (see [plan.md](plan.md) Project Structure)
- Feature fixtures under `specs/012-internode-and-time/contracts/fixtures/`

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Create the two new crates and lock the `internode` module layout this feature owns

- [ ] T001 Create `crates/clocks/Cargo.toml` (package `spacestorage-clocks`, edition 2024) and `crates/clocks/src/lib.rs` that `mod`s `stamp` and `skew`
- [ ] T002 Create `crates/replication/Cargo.toml` (package `spacestorage-replication`, edition 2024) and `crates/replication/src/lib.rs` that `mod`s `source_log`, `follower`, `stream`, `fence`
- [ ] T003 [P] Ensure `crates/internode` layout matches the plan in `crates/internode/src/{lib,frame,auth,heartbeat,rtt,backpressure,fanout,registry}.rs` (create missing modules as empty `mod` stubs owned by this feature)
- [ ] T004 Add `crates/clocks` and `crates/replication` to workspace `[workspace.members]` in `Cargo.toml` and declare path deps from `internode` / `node` / `placement` / `controlplane` as needed without introducing gRPC/`*-sys`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Shared frame, config, auth seam, validation codes, and node wiring that every user story needs

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [ ] T005 Implement length-prefixed frame encode/decode (`u32le length | u8 version | u16le msg_type | payload`) with current version **1**, accept **1 and 0**, refuse outside N/N+1 as `version_incompatible` in `crates/internode/src/frame.rs` (reuse from `replication` via path dep or shared call)
- [ ] T006 [P] Extend config grammar/validation in `crates/config` for required `entrypoint { handler internode; }` and `handler replication;`, default address `127.0.0.1`, starter ports 7000/7001, required `tls {…}` or `plaintext;`, illegal `disable internode;` / `disable replication;` (`internode_cannot_disable`), and live-reloadable `cluster.heartbeat_interval` (default `2s`), `cluster.failure_timeout` (default `15s`), `replication.max_stamp_skew` (default `500ms`), buffers `internode.{send,recv}` / `replication.{send,recv}` (default 64 MiB) per [config-directives.md](contracts/config-directives.md)
- [ ] T007 [P] Export/reuse validation and protocol codes in `crates/config` / `crates/internode` / `crates/replication` as needed: `internode_required`, `replication_required`, `transport_required`, `cluster_address_required`, `version_incompatible`, `stream_backpressured`, `quorum_domain_required`, `quorum_domain_unknown`, `quorum_domain_live_change`, `quorum_domain_change`, plus promote/write codes from later contracts
- [ ] T008 Implement join-secret constant-time verify handshake before cluster protocol in `crates/internode/src/auth.rs` (first binary: secret verify **is** the replication-role check; failure → close; unauthenticated peers refused)
- [ ] T009 Implement per-stream send/recv backpressure in `crates/internode/src/backpressure.rs` and mirror bounded buffers in `crates/replication/src/stream.rs` so a full buffer slows or fails **that** stream (`stream_backpressured`) and MUST NOT block a Tokio worker
- [ ] T010 Wire `InternodeService` and `ReplicationService` listeners from `crates/node` so both handlers bind before `ready` (default loopback); omit either → startup fail; shared port with `admin`/`admin-http`/tenant/each other → `entrypoint_port_conflict`
- [ ] T011 [P] Register additive metric series stubs (no `08` renames) in `crates/internode/src/lib.rs` / `crates/replication/src/lib.rs` / `crates/clocks/src/skew.rs` (or the `008` exposition hook they call): `spacestorage_internode_peers`, `spacestorage_internode_heartbeat_rtt_seconds`, `spacestorage_failure_detector_unavailable`, `spacestorage_hlc_skew_seconds`, `spacestorage_hlc_skew_over_limit`, `spacestorage_quorum_forward_total`, `spacestorage_promote_total`, `spacestorage_epoch_fenced_total` per [metrics.md](contracts/metrics.md)
- [ ] T012 [P] Mark `cluster.locality_key` obsolete for voting/HLC in `crates/placement` / `crates/config` (ladder remains farness; labels MUST NOT silently become a domain)

**Checkpoint**: Workspace builds; config rejects missing handlers/transport/disable; both ports can bind on loopback stubs. User stories can start.

---

## Phase 3: User Story 1 - Always-on cluster ports (Priority: P1) 🎯 MVP

**Goal**: Every node declares always-on `internode` and `replication` entrypoints (default loopback), requires `tls`/`plaintext;`, authenticates peers via join secret / replication role, refuses incompatible frame versions, and applies per-stream backpressure without blocking workers.

**Independent Test**: Start one node; confirm both listeners on loopback; omit transport and confirm startup error; connect without replication role/join secret and confirm refuse; connect with out-of-window version and confirm refuse.

### Tests for User Story 1 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T013 [P] [US1] Add conformance/fixture tests in `crates/conformance` (or `crates/node` tests) for [single-node.conf](contracts/fixtures/single-node.conf): node reaches `ready` with `internode` `:7000` and `replication` `:7001` accepting on `127.0.0.1` (SC-001)
- [ ] T014 [P] [US1] Add startup-fail tests in `crates/config/tests/` (or `crates/conformance`) for `specs/012-internode-and-time/contracts/fixtures/invalid/` omit-transport and omit-internodes/replication fixtures → `transport_required` / `internode_required` / `replication_required` (SC-001)
- [ ] T015 [P] [US1] Add auth refuse test: peer without join secret / replication role is closed before cluster protocol in `crates/internode` tests
- [ ] T016 [P] [US1] Add frame version window unit tests in `crates/internode/src/frame.rs`: accept 1 and 0; refuse outside window with `version_incompatible`
- [ ] T017 [P] [US1] Add backpressure unit test in `crates/internode/src/backpressure.rs`: full buffer → `stream_backpressured` on that stream; worker threads are not parked

### Implementation for User Story 1

- [ ] T018 [US1] Complete always-on `InternodeService` accept loop in `crates/internode/src/lib.rs` (listen even with zero peers; advertise listen address for `011` membership)
- [ ] T019 [P] [US1] Complete always-on `ReplicationService` accept loop in `crates/replication/src/lib.rs` (distinct port; JSON stream header then raw length-prefixed batches per [frame.md](contracts/frame.md))
- [ ] T020 [US1] Enforce remote join requires non-loopback cluster listen address (`cluster_address_required`) in `crates/config` / `crates/membership` when `cluster { join; }` targets a remote seed while handlers bind loopback
- [ ] T021 [US1] Implement message registry for US1 control types and unknown-`msg_type` → `Ack { error: unknown_message }` without tearing the mesh in `crates/internode/src/registry.rs`
- [ ] T022 [US1] Add admin/CLI `spacestorage fabric` listing internode/replication listen addresses in `crates/admin-proto` and `crates/spacestorage` per [admin-cli.md](contracts/admin-cli.md)
- [ ] T023 [US1] Ensure first-binary / release-profile starters declare both handlers (`plaintext;` on loopback) in `crates/release-profile` / docs examples consumed by `016` without disabling either handler

**Checkpoint**: Single-node always listens; transport/auth/version/backpressure behaviors match US1 acceptance scenarios.

---

## Phase 4: User Story 2 - Source domain vs log-follower (Priority: P1)

**Goal**: First-class `quorum_domain` (exactly one per member); bootstrap `default`; join names existing domain; container source vs async log-followers; writes in followers forward to source (no follower WAL); write levels count only durable source replicas; session/query fallback to `LOCAL_ONE` still needs source WAL; ordinary/force promote with epoch fence; `multi_active=on` create refused.

**Independent Test**: Bootstrap one node (default domain); join a second into that domain and a third into a newly created domain; write in A and in B; cut A; attempt ordinary promote vs force-promote; create with `multi_active=on` refused.

### Tests for User Story 2 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T024 [P] [US2] Add SC-008 domain membership tests in `crates/conformance`: bootstrap creates `default` with one member; join omit/unknown → refused; live domain-assign refused; member cannot sit in two domains; `region`/`planet` labels are not domains
- [ ] T025 [P] [US2] Add SC-002/SC-003/SC-004 write-path tests in `crates/conformance`: `TWO`/`QUORUM` count only durable source replicas; follower writes forward; no independent follower source log; undeclared fallback fails when source cannot meet level; `quorum_fallback=LOCAL_ONE` still requires source WAL; response names `met`
- [ ] T026 [P] [US2] Add SC-007 promote tests in `crates/conformance`: caught-up ordinary promote succeeds + fences; lagging + live source → ordinary refuse; force without `--accept-data-loss` refuse; force with accept succeeds; FD-unavailable source allows ordinary promote; two live sources never occur
- [ ] T027 [P] [US2] Add SC-005 `multi_active=on` create → `MultiActiveRefused` in `crates/types` / `crates/conformance`; catalog default off; ordered/log forced off

### Implementation for User Story 2

- [ ] T028 [P] [US2] Persist `QuorumDomain { name /* unique; bootstrap creates default */, members /* node_id appears in exactly one domain */ }` in cluster `ClusterStore` via `crates/controlplane` (empty domain MAY exist after decommission; delete of `default` refused while it is the only domain)
- [ ] T029 [US2] On bootstrap, create domain `default` and place the first node in it; refuse live `domain-assign` (`quorum_domain_live_change`) in `crates/controlplane` / `crates/membership`
- [ ] T030 [US2] Require `JoinRequest.quorum_domain` naming an existing domain in `crates/membership` (omit → `quorum_domain_required`; unknown → `quorum_domain_unknown`; store on `MemberRecord.quorum_domain`; replace into a different domain → `quorum_domain_change`)
- [ ] T031 [P] [US2] Add admin verbs `domain-create` and `domains` in `crates/admin-proto` and `crates/spacestorage` (`CLUSTER_ADMIN`; ladder keys are not auto-created as domains)
- [ ] T032 [US2] Model `ContainerReplication` in cluster/namespace catalog (`source_domain` exactly one; `follower_domains[]`; `epoch` starts at 1; `multi_active` default false; `each_quorum_policy` `refuse`|`wait`) in `crates/controlplane` / `crates/types`
- [ ] T033 [US2] Implement source-log assign/apply (`SourceLogPosition { container_id, epoch, position }`) in `crates/replication/src/source_log.rs` and follower apply-in-order with no independent follower source log in `crates/replication/src/follower.rs`
- [ ] T034 [US2] Implement in-domain `FanoutWrite`/`FanoutRead`/`Ack` and follower `ForwardWrite` in `crates/internode/src/fanout.rs` + coordinator path in `crates/exec` (follower MUST NOT ack from local WAL; ordered/log types forward to leader in A or `IndependentFollowerWrite`)
- [ ] T035 [US2] Count write `ONE`/`LOCAL_ONE`/`TWO`/`QUORUM` only durable replicas in `source_domain` in `crates/placement`; map `004` sync groups → source membership ∩ targets and async → followers; implement `EACH_QUORUM` as source quorum + follower apply or refuse when `each_quorum_policy=refuse`
- [ ] T036 [US2] Implement `WriteAttempt` outcome path in `crates/exec`: if source cannot meet requested level → `QuorumUnsatisfiable` unless session/query `quorum_fallback=LOCAL_ONE` (query then session only; never silent); fallback still needs one durable source WAL; response includes `met`
- [ ] T037 [US2] Implement promote/fence in `crates/replication/src/fence.rs` and controlplane: ordinary allowed if follower applied through last known source position **or** every old-source member is FD-unavailable; else `PromoteNotCaughtUp`; force requires `accept_data_loss` else `PromoteNeedsDataLossAccept`; success `epoch+=1`, fence old source, refuse two live sources (`TwoSources`); old-epoch writes → `EpochFenced`
- [ ] T038 [US2] Add CLI `spacestorage promote <container> --to <domain>` and `--force --accept-data-loss` in `crates/spacestorage` / `crates/admin-proto` per [promote.md](contracts/promote.md)
- [ ] T039 [US2] Refuse container create with `multi_active=on` as `MultiActiveRefused` in first binary in `crates/types` / catalog create path; force ordered/log types `multi_active=false`
- [ ] T040 [US2] Implement stronger reads forward to source when follower cannot satisfy; `LOCAL_ONE` read in follower MAY serve stale local apply in `crates/exec`
- [ ] T041 [P] [US2] Increment `spacestorage_quorum_forward_total{result}` from `crates/exec` forward path and `spacestorage_promote_total` / `spacestorage_epoch_fenced_total` from `crates/replication/src/fence.rs` / controlplane promote handler

**Checkpoint**: Two-domain forward/fallback/promote and one-domain-per-member rules pass SC-002–SC-005, SC-007–SC-008 independently of HLC skew/FD polish.

---

## Phase 5: User Story 3 - HLC in-domain, source log across domains (Priority: P1)

**Goal**: One HLC per domain this node is in; LWW-by-HLC in-domain; never compare HLC across domains; cross-domain copy follows source log; skew beyond `max_stamp_skew` degrades health without silent reorder; publish RTT/skew for planner override.

**Independent Test**: Concurrent writes in A converge by HLC (or type merge); concurrent apply on B follows source log (follower HLC MUST NOT win); inject skew past tolerance → `node_state=degraded`.

### Tests for User Story 3 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T042 [P] [US3] Add unit tests in `crates/clocks/src/stamp.rs`: tick never decreases `(physical, logical)` for this node; compare order physical → logical → `node_id` only when `domain_id` equal; cross-domain compare → `HlcCrossDomain`
- [ ] T043 [P] [US3] Add SC-006 conformance tests in `crates/conformance`: in-domain concurrent writes converge by LWW HLC (or declared merge); follower vs source disagreement resolved by source-log position; follower local HLC MUST NOT win
- [ ] T044 [P] [US3] Add skew health test in `crates/clocks/src/skew.rs` (or `crates/conformance`): inject `|physical_remote - physical_local| > 500ms` in-domain → `node_state=degraded` (`unhealthy_clock`) while writes still succeed

### Implementation for User Story 3

- [ ] T045 [P] [US3] Implement `HlcStamp { domain_id, physical_micros, logical, node_id }` and tick/compare APIs in `crates/clocks/src/stamp.rs` (tick: `physical = max(wall_micros, last.physical)`; equal → `logical += 1` else `logical = 0`)
- [ ] T046 [US3] Implement `DomainClock` with persist path `{data_dir}/clocks/<domain>.json` (`last`, `persisted_at`) loaded before `ready` so restart does not go backwards in `crates/clocks/src/lib.rs`
- [ ] T047 [P] [US3] Implement skew sampling and `max_stamp_skew` (default **500ms**) health signal in `crates/clocks/src/skew.rs`; over limit → `node_state=degraded` on observer; writes continue
- [ ] T048 [US3] Carry sender HLC on internodes `Heartbeat` and stamp leaderless writes in-domain in `crates/internode/src/heartbeat.rs` / write path; re-export former `placement/stamp.rs` from `spacestorage-clocks`
- [ ] T049 [US3] Default in-domain conflict = LWW by HLC in `crates/placement` / type merge hook; optional type `ConflictMerge` in `crates/types`; record disagreement and correct stale replicas via `crates/internode` coordination + `crates/replication` bytes; ordered/log types MUST NOT use LWW to merge two leadership histories (`006`)
- [ ] T050 [US3] Ensure cross-domain apply uses source-log sequence only (callers MUST NOT compare HLC across domains) in `crates/replication/src/follower.rs`
- [ ] T051 [P] [US3] Publish RTT EMA (`crates/internode/src/rtt.rs`) and HLC skew samples for `004`/`005` ladder-rank override once samples exist
- [ ] T052 [P] [US3] Add CLI `spacestorage clocks` (per-domain HLC, skew samples, `max_stamp_skew`) and ensure `spacestorage health` surfaces skew-degraded `node_state` in `crates/spacestorage`
- [ ] T053 [P] [US3] Increment `spacestorage_hlc_skew_seconds` and `spacestorage_hlc_skew_over_limit` gauges from `crates/clocks/src/skew.rs`

**Checkpoint**: SC-006 and skew health hold; HLC never compared across domains.

---

## Phase 6: User Story 4 - Failure detection and partitions (Priority: P2)

**Goal**: Cluster-wide fixed heartbeat timeout on `internode` marks peers unavailable for quorum **and** replace-eligible; still-heartbeating peers count; minority controllers remain CP; repair/hint/anti-entropy **bytes** move on `replication` streams throttleable by `004` policy.

**Independent Test**: Stop a peer; after cluster-wide timeout it does not count and replace is allowed; restore heartbeat and confirm it counts again; confirm repair stream path exists.

### Tests for User Story 4 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T054 [P] [US4] Add SC-009 FD tests in `crates/conformance` / `crates/internode`: after `failure_timeout` (default 15s) silent peer is unavailable for quorum and replace-eligible; still-heartbeating peer still counts and is not replaceable; two observers agree on the same cluster-wide timeout
- [ ] T055 [P] [US4] Add unit tests in `crates/internode/src/heartbeat.rs` for `FailureDetectorView` status `alive`|`unavailable` iff `now - last_heartbeat >= cluster.failure_timeout`
- [ ] T056 [P] [US4] Add stream smoke test that repair/hint/bootstrap bytes use `replication` (`RepairChunk`/`HintReplay`/`Stream*`) while coordination stays on internodes in `crates/replication` tests

### Implementation for User Story 4

- [ ] T057 [US4] Implement cluster-wide failure detector in `crates/internode/src/heartbeat.rs` (`heartbeat_interval` default 2s; `failure_timeout` default 15s; live-reloadable; **no** phi-accrual; **no** per-observer/per-group override — remove `004` per-destination-group FD for this event)
- [ ] T058 [US4] Publish FD unavailable event to quorum counting (`crates/placement`) and replace eligibility (`crates/membership` / `011`); heartbeat resume → available again unless decommissioned/fenced
- [ ] T059 [US4] Confirm minority controller partition cannot mutate membership/schema in `crates/controlplane` / `crates/internode` Raft path (consume `006` CP; no data-path auto-promote on FD in `crates/replication`)
- [ ] T060 [US4] Implement replication stream movers in `crates/replication/src/stream.rs`: `SourceLogBegin/Record/End`, `StreamBegin/Chunk/End`, `RepairChunk`, `HintReplay`; honor `004` `repair_bytes_per_sec` / `rebalance_bytes_per_sec`; coordination `RepairBegin/End` stays on internodes
- [ ] T061 [P] [US4] Expose peer FD status on `spacestorage fabric` in `crates/spacestorage` / `crates/admin-proto` and set `spacestorage_failure_detector_unavailable` / heartbeat RTT metrics from `crates/internode/src/heartbeat.rs`
- [ ] T062 [US4] On domain heal after split, ensure LWW/HLC (or type merge) converges in `crates/placement` / `crates/replication` and epoch fence in `crates/replication/src/fence.rs` still refuses two source domains

**Checkpoint**: SC-009 passes; repair bytes path exists without mixing bulk copy onto Raft/heartbeat buffers.

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: Docs, quickstart validation, first-binary wiring, cleanup

- [ ] T063 [P] Sync starter/invalid fixtures under `specs/012-internode-and-time/contracts/fixtures/` with implemented validation codes and document defaults (15s FD, 500ms skew, ports 7000/7001) in `docs/` examples if present
- [ ] T064 [P] Run and fix `specs/012-internode-and-time/quickstart.md` scenarios 1–5 against the in-process harness in `crates/conformance` (SC-001–SC-009 gates)
- [ ] T065 Ensure `016` first-binary slices 1–5 require internodes + replication + one `quorum_domain` without inventing a second client protocol in `crates/release-profile` / conformance feature flags
- [ ] T066 [P] Code cleanup in `crates/placement` / `crates/config`: remove obsolete `locality_key` voting uses; ensure placement calls `crates/clocks` / `crates/replication` for “who counts” and streams only
- [ ] T067 Verify performance smoke on loopback in `crates/conformance` (or `crates/internode` benches): heartbeat RTT goal &lt; 5 ms p95; source-domain write at `TWO` does not wait on follower RTT (cross-check `004` SC-010 harness if present)
- [ ] T068 [P] Final `cargo test -p spacestorage-clocks -p spacestorage-internode -p spacestorage-replication` and `cargo test -p spacestorage-conformance` for SC-001–SC-009 (run from repository root)

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately
- **Foundational (Phase 2)**: Depends on Setup completion — **BLOCKS** all user stories
- **User Stories (Phases 3–6)**: All depend on Foundational completion
  - US1 (ports) has no story dependency — suggested MVP
  - US2 (domain/source/follower) needs US1 listeners/auth for real mesh tests; domain object work can start after Foundational
  - US3 (HLC) needs Foundational; heartbeat carriage integrates with US1; LWW integrates with US2 write path
  - US4 (FD/streams) needs US1 heartbeats; replace/quorum consumers need US2 who-counts; repair bytes independent of promote
- **Polish (Phase 7)**: Depends on all desired user stories being complete

### User Story Dependencies

- **User Story 1 (P1)**: After Foundational — no dependency on other stories — 🎯 MVP
- **User Story 2 (P1)**: After Foundational; mesh tests expect US1 ports/auth; promote ordinary path that keys off FD-unavailable expects US4 detector (force/caught-up paths testable without US4)
- **User Story 3 (P1)**: After Foundational; full SC-006 with followers expects US2 source log; skew uses US1 heartbeats
- **User Story 4 (P2)**: After Foundational; quorum/replace integration expects US2; stream types usable once US1 replication port exists

### Within Each User Story

- Tests (listed) MUST be written and FAIL before implementation
- Models/entities before services
- Services before admin/CLI and integration
- Story complete before moving to next priority when staffing is serial

### Parallel Opportunities

- T001/T002/T003 in Setup (different crates/files)
- T006/T007/T011/T012 in Foundational after T005 frame exists (T008–T010 more serial on node wiring)
- All US1 test tasks T013–T017 in parallel; T019 parallel with T018 once frame/auth ready
- All US2 test tasks T024–T027 in parallel; T028/T031/T032 model work in parallel before write path
- All US3 test tasks T042–T044; T045/T047 stamp+skew in parallel
- All US4 test tasks T054–T056; metrics/CLI polish tasks marked [P]
- After Foundational, different developers can own US1 vs US3 clocks crate vs US2 domain model with planned integration points

---

## Parallel Example: User Story 1

```bash
# Launch all US1 tests together:
Task: "Add conformance/fixture tests for single-node.conf (SC-001)"
Task: "Add startup-fail tests for omit-transport / omit-handlers"
Task: "Add auth refuse test for missing join secret"
Task: "Add frame version window unit tests"
Task: "Add backpressure unit test"

# Launch parallel implementation pieces:
Task: "Complete ReplicationService accept loop"
Task: "Add spacestorage fabric CLI"
```

## Parallel Example: User Story 2

```bash
# Launch all US2 tests together:
Task: "SC-008 domain membership tests"
Task: "SC-002/003/004 write-path tests"
Task: "SC-007 promote tests"
Task: "SC-005 multi_active refuse tests"

# Launch parallel model/admin pieces before write path:
Task: "Persist QuorumDomain in ClusterStore"
Task: "Add domain-create / domains admin verbs"
Task: "Model ContainerReplication catalog fields"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL — blocks all stories)
3. Complete Phase 3: User Story 1 (always-on ports, auth, version, backpressure)
4. **STOP and VALIDATE**: Independent Test for US1 / SC-001
5. Demo single-node `fabric` listeners if ready

### Incremental Delivery

1. Setup + Foundational → foundation ready
2. Add US1 → always-on fabric (MVP)
3. Add US2 → default domain + source/follower + promote + `multi_active` refuse (needed for first-binary slice 5 with US1)
4. Add US3 → HLC LWW + skew health
5. Add US4 → cluster-wide FD + repair byte streams
6. Polish → quickstart + conformance SC-001–SC-009 green

### Parallel Team Strategy

1. Team completes Setup + Foundational together
2. Once Foundational is done:
   - Developer A: User Story 1 (internode/replication listeners)
   - Developer B: User Story 3 (`crates/clocks` + skew) then integrate heartbeats
   - Developer C: User Story 2 (domain + source/follower + promote)
3. Developer A/C then finish User Story 4 (FD on heartbeat + replication streams)
4. Stories integrate at documented seams (heartbeat carries HLC; FD feeds promote ordinary path and `011` replace)

---

## Notes

- [P] tasks = different files, no dependencies on incomplete tasks
- [Story] label maps task to specific user story for traceability
- Each user story should be independently completable and testable at its Independent Test
- Commit after each task or logical group
- Stop at any checkpoint to validate story independently
- Do not implement membership bootstrap/join/leave (`011`), placement RF/anti-affinity (`004`), or durable-ack disk meaning (`013`) beyond the seams this feature owns
- Avoid: vague tasks, same-file conflicts, treating `planet`/`region` as a `quorum_domain`, follower local WAL, phi-accrual FD, cross-domain HLC compare
