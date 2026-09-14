# Implementation Plan: Distribution, Placement, Media, and Replication (L1 Shared Capabilities)

**Branch**: `004-distribution-placement` | **Date**: 2026-09-14 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/004-distribution-placement/spec.md` (with Clarifications, Session 2026-09-14)

## Summary

Replace the interim single-node `PlacementDirector` from `003` (`crates/placement/src/local.rs`, replicas = 1) with the real L1 execution layer: a cluster-wide **topology** of labelled nodes, drives and memory pools; **constraint-satisfying placement** of replicas with strict anti-affinity; **leaderless** request coordination from any node; **Cassandra-style quorum** whose write acknowledgements are durable according to the container's storage mode; **sync and async destination groups**; **sharding and partitioning**; **failure detection, hinted repair and rebalancing**; and a **two-phase commit** for the distributed-transactions capability.

`crates/placement` stays the crate `003` already depends on. This feature fills it: topology, label selectors, the placement planner, replica-set state, quorum arithmetic (including the container level of the `002` precedence chain), destination-group replication, shard/partition maps, repair and rebalance plans. A new `crates/internode` crate adds a dedicated `internode` handler (its own port, like `admin`) so coordinators can fan out mutations and reads without inventing a second client protocol. Cluster membership and the placement catalog are stored behind a `ClusterStore` seam that `06` will front with Raft; until then every configured peer shares an append-only placement log over internodes.

Clarifications baked in: leaderless data path (Q1); last-writer-wins by hybrid logical clock stamp, with type-supported alternatives (Q2); persistent/hybrid write quorum counts only drive-backed acknowledgements (Q3); `EACH_QUORUM` on an asynchronous remote group is refused unless the container opts into waiting (Q4); unsatisfiable anti-affinity is refused, not best-effort placed (Q5).

## Technical Context

**Language/Version**: Rust 1.87 (edition 2024, MSRV 1.85), same workspace as `001`–`003`.

**Primary Dependencies** (all pure Rust; `*-sys` prohibited, `cargo-deny` as in `003`): existing workspace crates (`tokio`, `async-trait`, `serde`/`serde_json`, `bytes`, `tracing`, `uuid`, `chrono`, `parking_lot`, `crc32fast`, `xxhash-rust`). No new C-backed crates. Internode framing reuses the length-prefixed envelope style of `001`'s admin TCP (no gRPC). Placement planning and consistent hashing are in-house (rendezvous / jump hash). Hybrid logical clocks for version stamps are in-house. Dev-dependencies: `proptest` (selector parsing, stamp ordering, shard-map bijection), `tempfile`, the `002` conformance client crates for any-node smoke.

**Storage**: Topology and placement plans are cluster metadata in the `003` catalog store (`catalog/placement/` log + snapshot), replicated to peers through internodes until `06` owns the log. Replica data stays in `003` storage under `ns/<namespace>/<container-id>/`; this feature never introduces a second on-disk format for user data. Hinted handoff and repair staging use `catalog/hints/` and `catalog/repair/` under the same `data_dir`. Memory-mode replicas allocate from the node's declared `memory { size }` / `types.memory` pool.

**Testing**: `cargo test`. Unit tests in `placement` (selector, planner, quorum arithmetic, stamp compare, shard map, rebalance plan invariants) and `internode` (frame codec, fan-out, timeout). `crates/conformance` gains a **multi-node** harness that boots three (and later six) in-process nodes with internodes on ephemeral ports: topology view (SC-001/002), anti-affinity matrix including refusals (SC-003), media constraints (SC-004), quorum vocabulary and durable-ack counting (SC-005/006), coordinator-independence (SC-007), one-replica-down (SC-008), partition/heal + LWW (SC-009), local-vs-remote latency (SC-010), shard bijection during split (SC-011/012), add/remove/relabel node (SC-013/014), 2PC with a killed participant (SC-015), refusal shape (SC-016), counters present (SC-017), all 13 capabilities executed or refused (SC-018). Contract tests validate every fixture under `contracts/fixtures/`.

**Target Platform**: Linux x86_64/aarch64 servers (primary), macOS for development. Multi-node tests run in one process with loopback internodes; latency-shaped tests inject a delay on the internodes path rather than requiring two machines.

**Project Type**: Cargo workspace extension — one new library crate (`internode`), `placement` grows from seam to engine, `config`/`node`/`admin-proto`/`spacestorage`/`conformance`/`exec` gain surfaces. No new binaries.

**Performance Goals**: placement of a new container < 50 ms p95 once topology is in memory; point write at `TWO` on a 3-replica loopback cluster ≥ 20 k ops/s per coordinator; internodes heartbeat round-trip < 5 ms p95 on loopback; rebalance of 1 GiB between two loopback nodes at the configured rate ± 20 %; local `LOCAL_QUORUM` write on a two-region fixture with 200 ms injected remote delay does not include that delay (SC-010); catalog/topology read < 1 ms.

**Constraints**: no blocking I/O on Tokio workers — internodes is fully async; CPU-heavy repair compare and rebalance hashing on the bounded `spawn_blocking` pool already used by `003` compaction; leaderless data path (Clarification Q1) — internodes fan-out is not a write-leader proxy; durable write acknowledgements only from drive-backed replicas for persistent/hybrid containers (Q3); anti-affinity never silently downgraded (Q5); internodes handler must be explicitly enabled like `admin` (`001` FR-015); `PlacementDirector` trait grows only with default-bodied methods so `003` keeps compiling; `PlacementInfo` (`002`) is implemented against live replica sets, not the stub of `replicas() == 1`.

**Scale/Scope**: 13 L1 capabilities executed; quorum vocabulary of 8 named levels + `Acks(n)`; label selector with equality/inequality/set/presence and boolean combinators; destination groups; hash and range/time partitions; hinted handoff + full-compare repair; rebalance planner; 2PC; internodes RPC of ~15 message kinds. Largest items: placement planner + topology (~4 k), replication/quorum/coordinator (~5 k), internodes (~2.5 k), shard/partition maps (~2 k), repair + rebalance (~3.5 k), 2PC (~1.5 k), config/admin/CLI (~1.5 k); ≈ 25–35 k lines including tests.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Gate | Status |
|---|-----------|------|--------|
| I | Rust-Only Stack | No new `*-sys` crates; internodes is a framed Tokio protocol, not gRPC/protobuf-c; hashing and HLC are in-house | PASS |
| II | Fully Asynchronous Tokio Runtime | Internodes, heartbeats, fan-out, repair and rebalance are `async`; compare/hash on the existing bounded `spawn_blocking` pool | PASS |
| III | Single-Process Multithreaded Monolith | `internode` and `placement` are libraries linked into `spacestoraged`; no sidecar cluster manager | PASS |
| IV | Type-Driven Multiparadigm | L1 remains a capability layer over types; this feature executes declarations `003` already validates. No parallel type hierarchy. Replication unit and conflict alternative come from the type descriptor (FR-044) | PASS |
| V | Protocol Compatibility on Distinct Ports | Internodes is an *internal* handler on its own port, enabled the same way as `admin`; client protocols are unchanged. Quorum semantics flow through `002`'s existing option surfaces | PASS |
| VI | Every Node Is a Request Coordinator | Clarification Q1: any node coordinates; replicas accept writes independently; internodes is fan-out, not a mandatory proxy | PASS |
| VII | Label-Based Planetary Placement | This feature **is** the principle: open-ended labels, drive-derived media labels, explicit memory pools, strict anti-affinity, locality as a label key, starter examples through planetary-scale timing knobs (FR-073, FR-075) | PASS |
| VIII | Cassandra-Style Quorum with Protocol Defaults | Owns level arithmetic and durable-ack counting; adds container to the `002` precedence chain; defaults remain write `TWO` / read `ONE`; `EACH_QUORUM` vs async remote is refuse-by-default (Q4) | PASS |
| IX | Multi-Tenant Namespaces | Placement is per container, hence per namespace; counters labelled with namespace for `07`/`08` | PASS |
| X | Observability as a Product Surface | FR-081 counters (acks required/achieved, quorum refusals, lag, repair volume, rebalance volume) in `node::stats` with `08`-conformant reserved names; no `08` series renamed | PASS |
| XI | Documented, Expandable Configuration | `labels`, `storage.drive`, `cluster`, `replication` blocks; six starter fixtures (FR-075); selector and capability docs with examples | PASS |
| XII | Raft Controller Elections and Local Restore | Does **not** run Raft. Consumes `06` elections where a type needs a leader (FR-078/079). Local restore of replica state remains `003`; this feature restores placement metadata from the placement log and re-establishes internodes sessions | PASS |
| XIII | Security Defaults for Data and Roles | Internodes authenticates with a cluster-shared token file (path reference, never inlined), TLS optional on the internodes entrypoint using `001`'s cert-path rules. No new role system (`07`) | PASS with justified deviation |
| Arch. Contracts | Five-level stack; protocol adapter ≠ type system | Placement executes L1 only; drivers still go through `002` → `exec` → coordinator. Quorum/timeout/namespace remain expressible everywhere | PASS |
| Observability Contract | No `08` label renamed or dropped | Additions only: `container`, `capability`, `quorum`, `ack_kind`, `region`/`locality` alongside existing `namespace`/`node` | PASS |

**Gate result (pre-research)**: PASS. Deviations justified in Complexity Tracking: interim `ClusterStore` (sequencing vs `06`), internodes shared token (XIII until `07`), interim 2PC participant log on the placement catalog (sequencing vs `06`).

## Project Structure

### Documentation (this feature)

```text
specs/004-distribution-placement/
├── plan.md                                  # This file
├── research.md                              # Phase 0: R1–R20
├── data-model.md                            # Phase 1: topology, placement, replica, quorum, groups, shards, repair, rebalance, 2PC
├── quickstart.md                            # Phase 1: 3-AZ cluster walkthrough mapped to every SC
├── contracts/
│   ├── topology.md                          # nodes, labels, drives, memory pools, domains (FR-001–FR-010)
│   ├── label-selector.md                    # constraint language (FR-011–FR-015)
│   ├── placement.md                         # planner, reports, persistent placement, director (FR-016–FR-027)
│   ├── quorum.md                            # levels, arithmetic, durable acks, precedence (FR-028–FR-037)
│   ├── replication.md                       # destination groups, sync/async, LWW stamps, lag (FR-038–FR-044, FR-071–FR-074)
│   ├── coordinator.md                       # internodes RPC, any-node fan-out (FR-045–FR-049)
│   ├── sharding-partitioning.md             # maps, routing, split (FR-050–FR-056)
│   ├── failure-repair.md                    # detection, hints, compare, partitions (FR-057–FR-064)
│   ├── rebalancing.md                       # plans, throttle, resume (FR-065–FR-070)
│   ├── distributed-transactions.md          # 2PC, visibility of in-doubt (FR-076–FR-079)
│   ├── config-directives.md                 # labels, drives, cluster, replication, internodes, buffers, CLI/admin
│   └── fixtures/
│       ├── README.md
│       ├── node-single.conf
│       ├── node-rack.conf
│       ├── cluster-3az/{db-1,db-2,db-3}.conf
│       ├── cluster-mixed-media/{nvme-1,hdd-1}.conf
│       ├── cluster-two-region/{eu-1,eu-2,us-1}.conf
│       └── invalid/*
├── checklists/requirements.md
└── tasks.md                                 # Phase 2 (/speckit-tasks — NOT created here)
```

### Source Code (repository root)

```text
Cargo.toml                                   # workspace: adds internode
crates/
├── config/                                  # (001–003) + labels{}, storage.drive{}, cluster{}, replication{}, internodes auth
├── node/                                    # + Topology, ClusterDirector wiring, internodes session, per-capability stats
├── admin-proto/                             # + TopologyView, PlacementReport, ReplicaSet, RebalancePlan, RepairStatus DTOs
├── spacestorage/                            # + topology, placement, replicas, rebalance, repair, cluster commands
├── exec/                                    # (002) LocalEngine routes mutations/reads through Coordinator; PlacementInfo is live
│
├── placement/                               # spacestorage-placement — CORE (003 seam, now the engine)
│   └── src/
│       ├── lib.rs                           # PlacementDirector (003 signatures + additive methods)
│       ├── matrix.rs                        # unchanged owner: 003 compatibility (this feature only consumes it)
│       ├── director.rs                      # ClusterDirector replaces LocalDirector as default
│       ├── local.rs                         # retained for single-node / tests; replicas = 1 still valid
│       ├── topology.rs                      # Node, Label, Drive, MemoryPool, TopologyView, domains
│       ├── selector.rs                      # LabelSelector parse/match
│       ├── planner.rs                       # constraint + anti-affinity + load; deterministic; refusal codes
│       ├── replica.rs                       # Replica, ReplicaSet, health, sync state
│       ├── quorum.rs                        # level arithmetic, durable-ack filter, clamp/reject
│       ├── stamp.rs                         # HLC version stamp, LWW compare, skew gauge
│       ├── group.rs                         # DestinationGroup, sync/async, EACH_QUORUM policy
│       ├── shard.rs                         # ShardMap, PartitionMap, key routing
│       ├── repair.rs                        # hints, replay, full compare
│       ├── rebalance.rs                     # plan, throttle, resume token
│       ├── txn.rs                           # 2PC coordinator/participant
│       └── error.rs                         # PlacementError codes (FR-082)
│
├── internode/                               # spacestorage-internode
│   └── src/
│       ├── lib.rs                           # InternodeHandler (001 Handler), PeerSet
│       ├── frame.rs                         # length-prefixed messages
│       ├── rpc.rs                           # FanoutWrite, FanoutRead, Hint, Repair, Rebalance, Heartbeat, CatalogDelta, Txn*
│       ├── heartbeat.rs                     # failure detector
│       └── auth.rs                          # shared token + optional TLS (001 cert paths)
│
├── types/ storage/                          # unchanged owners; placement calls storage per replica locally
├── conformance/
│   └── tests/
│       ├── topology_view.rs                 # SC-001, SC-002
│       ├── anti_affinity.rs                 # SC-003, Q5
│       ├── media_constraints.rs             # SC-004
│       ├── quorum_levels.rs                 # SC-005, SC-006, Q3 durable acks
│       ├── coordinator_any_node.rs          # SC-007, Q1
│       ├── replica_down.rs                  # SC-008
│       ├── partition_heal.rs                # SC-009, Q2 LWW
│       ├── local_async_remote.rs            # SC-010, Q4 EACH_QUORUM refuse
│       ├── shards.rs                        # SC-011, SC-012
│       ├── rebalance_lifecycle.rs           # SC-013, SC-014
│       ├── two_phase_commit.rs              # SC-015
│       ├── refusal_shape.rs                 # SC-016
│       ├── placement_stats.rs               # SC-017
│       └── capabilities_executed.rs         # SC-018
│
docs/
├── placement.md                             # labels, selectors, anti-affinity, media, memory
├── replication.md                           # quorum, groups, LWW, lag
└── examples/                                # copies of the six starter fixtures
```

**Structure Decision**: Keep `crates/placement` as the crate `003` already depends on so `TypeSystem` continues to call `PlacementDirector` without a second capability crate. Split internodes into its own crate so the handler can register in `001`'s `HandlerRegistry` without `placement` depending on TCP. `LocalDirector` remains for single-node tests and for a node started without internodes (replication factor 1 only). `exec` grows a `Coordinator` that uses `PlacementInfo` live data; `05` will replace query planning but will keep this fan-out.

## Complexity Tracking

| Violation / deviation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| Interim `ClusterStore` (sequencing vs `06`) | Placement, replica health and 2PC participant state must be cluster-wide now; Raft controllers are `06` | Inventing Raft here would violate Principle XII ("this feature consumes elections, never runs a second protocol"). The append-only log + internodes delta is local-plus-peers, and `ClusterStore` is the seam `06` fronts. |
| Internodes shared token file (XIII until `07`) | Peers must authenticate; role system is `07` | Leaving internodes unauthenticated would put replica traffic on the network with only `001` admin token isolation. Token is a path reference, `0600`, never inlined; `07` replaces `InternodeAuth` the same way it replaces `Authenticator`. |
| 2PC participant log on the placement catalog (sequencing vs `06`) | FR-076/077 require durable single-outcome commit now | Deferring transactions would leave a named L1 capability unimplemented (SC-018). The log is the same catalog `06` will replicate; isolation and SQL syntax stay `05`. |
| In-house internodes instead of gRPC | Principle I forbids C-backed HTTP/2 stacks; `001` already has a framed admin TCP | Adding `tonic`/`prost` pulls non-Rust code generation and a second RPC style. Message kinds are few and stable. |

## Phase 0 — Research

See [research.md](research.md), R1–R20. All Technical Context items are resolved; no `NEEDS CLARIFICATION` remains. Key outcomes: crate split and additive `PlacementDirector` (R1); config-declared topology plus live admin mutations (R2); internodes catalog delta until `06` (R3); deterministic greedy planner (R4); selector grammar (R5); durable-ack quorum (R6); HLC stamps (R7); destination groups and Q4 (R8, R9); hinted handoff window 3 h (R10); rendezvous hashing (R11); range/time partitions (R12); constraint-safe rebalance (R13); 2PC (R14); internodes handler (R15); reserved config blocks (R16); operational defaults that do not assume a low RTT (R17); fixed heartbeat timeout, not phi-accrual (R18); leaderless vs datatype primary (R19); catalog-driven multi-node conformance (R20).

## Phase 1 — Design

- [data-model.md](data-model.md): topology, selector, placement, replica set, destination group, quorum decision, shard/partition maps, repair, rebalance, 2PC, internodes peer, execution record extensions; state machines for replica health, rebalance, and 2PC.
- [contracts/](contracts/): eleven contracts plus starter and invalid fixtures covering FR-001–FR-083.
- [quickstart.md](quickstart.md): three-AZ cluster from the starter fixtures, then media, failure, shards, two-region async, and rebalance — mapped to every success criterion.

## Constitution Check (post-design)

Re-evaluated after Phase 1: no new violations. Design keeps internodes as a library handler on its own port; placement as L1 execution rather than a second database; leaderless coordination; quorum defaults and durable-ack filter; open-ended labels with strict anti-affinity; no Raft here; counters additive for `08`; documented starters. The three interim seams stand as tracked above. **Gate result: PASS.**

## Risks and sequencing notes

- **Multi-node conformance is the schedule.** Suggested waves for `/speckit-tasks`: (1) topology + selector + config/admin/CLI + internodes heartbeat, so a 3-node cluster describes itself; (2) planner + replica sets + `ClusterDirector` replacing `LocalDirector` as default when internodes is enabled; (3) coordinator fan-out + quorum arithmetic + durable acks + HLC; (4) destination groups, lag, Q4 `EACH_QUORUM`; (5) failure detector, hints, repair, partition/heal; (6) sharding/partitioning; (7) rebalance; (8) 2PC; (9) docs, starters, SC-018 sweep.
- **`003` interface growth**: `PlacementDirector` new methods need default bodies (no-op or `LocalDirector` behaviour) or type-system tests break. CI builds `003` crates against each `placement` commit.
- **`002` `PlacementInfo`**: `replicas()` becomes live; default-clamping tests that assumed `replicas()==1` must run against `LocalDirector` (no internodes) *and* against a 3-replica cluster. Explicit `TWO` on a single-node node still rejects.
- **`05` will own the planner** but must keep this coordinator: `LogicalRequest` fan-out is placement's job; join/aggregate planning is not.
- **Clock skew**: HLC plus a skew gauge; beyond tolerance (default 500 ms, configurable) the node is marked unhealthy for LWW-sensitive containers rather than silently rewriting history.
- **Single-node without internodes** remains supported: `LocalDirector`, RF=1, any RF>1 or anti-affinity declaration refused with "internodes required".

## Next Step

Run `/speckit-tasks` to generate `tasks.md` from this plan and the Phase 1 artifacts.
