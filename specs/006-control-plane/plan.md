# Implementation Plan: Control-Plane Hierarchy, Raft Elections, and Node Restore

**Branch**: `006-control-plane` | **Date**: 2026-09-16 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/006-control-plane/spec.md` (Clarifications, Session 2026-09-16 — dedicated odd-sized voter set with migration and optional exclusive data; one Raft per namespace; leadership leases with epoch fencing; first-binary cluster+namespace Raft; namespace primary as metrics aggregator)

## Summary

Replace the interim `ClusterStore` shipper from `004`/`011` (append-only log + internodes `CatalogDelta`, LWW by HLC) with **in-process Raft** on the existing `internode` handler. Two electing levels: one **cluster** Raft group (membership, namespace list, nodes, drives, role store) and **one Raft group per namespace** (schemas, container definitions, shared-datatype metadata). Votes are cast only by a **dedicated odd-sized voter set**, not every member. Datatype-level is **not** a Raft group: ordered types take a **leadership lease** (epoch-fenced) from the namespace group; leaderless KV/SQL/document writes never wait on it.

This crate implements the `ClusterStore` trait so placement, membership, and 2PC keep their log format and apply path. Raft is the replication of that log. Local restore (`FR-009`) is an orchestrator: definitions from the applied Raft/catalog state, durable content via `013`, memory-mode empty unless `004` re-populates.

**First binary** (`16` slices 1–5, tightened by `FR-020`): cluster Raft + a namespace Raft for every namespace that exists; static voter set (1 on bootstrap, 3 after the third member); exclusive-data off; no voter migration; no ordered-type leases required. **Slice 7**: voter replace/grow/shrink, exclusive-data option, plus `07`/`14` consumers of the role blob already stored here.

## Technical Context

**Language/Version**: Rust 1.87 (edition 2024, MSRV 1.85), same Cargo workspace as `001`–`016`.

**Primary Dependencies**: existing workspace crates (`tokio`, `async-trait`, `serde`/`serde_json`, `bytes`, `tracing`, `uuid`, `parking_lot`) plus **`openraft`** (pure Rust, Tokio). No gRPC, no protobuf-c, no `*-sys`, no etcd/Consul sidecar. Internode framing stays the `004` length-prefixed envelope. HLC stamps on metadata records come from `012` (already in internodes heartbeats). Fsync of Raft log/snapshot via the existing bounded `spawn_blocking` pool (`013` / constitution II).

**Storage**: Per Raft group under `{data_dir}/raft/<group_id>/`: log segments + snapshot + format version. Group ids: `cluster` and `ns/<namespace_id>`. This is **controller metadata**, not tenant WAL (`013` remains one WAL per node/drive for container content). Applied state feeds the existing `003` catalog and `004` `ClusterStore` snapshot. Memory-mode tenant content is not stored here.

**Testing**: `cargo test`. Unit tests in `controlplane` (voter-set parity, joint config odd-size refuse, lease epoch, NotLeader forward). `crates/conformance` gains in-process 1/3/5-node harness: election (SC-001/002), minority refuse (SC-003), restore split (SC-004), leaderless KV without lease (SC-005), secondary metadata read (SC-007), election metrics (SC-008), aggregator identity (SC-011). Feature `controlplane-ops`: voter migrate (SC-009) and exclusive-data (SC-010). Feature `controlplane-leases` (or ordered-type present): epoch fence (SC-006). Contract tests on `contracts/fixtures/`.

**Target Platform**: Linux x86_64/aarch64 servers (primary), macOS for development. Multi-node tests: in-process nodes, loopback internodes, same harness as `004`/`016`.

**Project Type**: Cargo workspace extension — **one new library crate** `crates/controlplane` (`spacestorage-controlplane`). No new binary. Implements `004`'s `ClusterStore`. Additive internodes message types. Config/admin/CLI surfaces. `release-profile`: first binary compiles cluster+namespace Raft; `controlplane-ops` is slice 7.

**Performance Goals**: cluster metadata write (create namespace) < 50 ms p95 on 3-voter loopback; metadata read on a secondary < 5 ms p95 after read-index; election after primary kill < `election_timeout` × 2 on loopback (fixture timeouts, not LAN-hardcoded); restore of definitions before `ready` within the `001` ready bound once `013` replay finishes; leaderless KV p95 unchanged vs `004` (no extra RPC to a datatype primary).

**Constraints**: Raft RPCs only on `internode` (`FR-014`). Minority of a **voter set** cannot mutate that group's store. Non-voters still coordinate client data. Leaderless writes MUST NOT take a lease. Stale-epoch appends refused. Default **co-locate** tenant data on controller voters; exclusive-data off until slice 7. Fsync not on Tokio workers. Metadata versions carry HLC (`FR-015`) in addition to Raft index. No second client protocol. No etcd. First binary MUST NOT require Log Stream.

**Scale/Scope**: 1 cluster group + N namespace groups; voter sets of 1/3/5 typical; learners = other members that need the log. ~10 internodes Raft RPC kinds; lease table; restore hook; admin describe/forward. Roughly: raft adapter+store (~4 k), groups+voters (~2 k), apply/catalog (~2 k), lease+metrics (~1.5 k), config/admin (~1 k), conformance (~3 k); ≈ 12–16 k lines including tests. First-binary path is cluster + one namespace, no ops/leases.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Gate | Status |
|---|-----------|------|--------|
| I | Rust-Only Stack | `openraft` is pure Rust; internodes framing reused; no etcd C client | PASS |
| II | Fully Asynchronous Tokio Runtime | Raft tick/RPC/apply are `async`; log fsync on `spawn_blocking` | PASS |
| III | Single-Process Multithreaded Monolith | `controlplane` is a library in `spacestoraged`; no controller sidecar | PASS |
| IV | Type-Driven Multiparadigm | Does not add types. Leadership lease is requested from a type descriptor (`003`/`004` FR-078), not a parallel engine | PASS |
| V | Protocol Compatibility on Distinct Ports | No new client port. Raft on existing `internode` | PASS |
| VI | Every Node Is a Request Coordinator | Non-voters and non-primaries still accept client data (FR-008) | PASS |
| VII | Label-Based Planetary Placement | Voter set is not the topology ladder. Exclusive-data is a placement exclusion flag consumed by `04` | PASS |
| VIII | Cassandra-Style Quorum with Protocol Defaults | Data path stays leaderless + quorum. Controllers are CP (Raft). No change to write TWO / read ONE | PASS |
| IX | Multi-Tenant Namespaces | One Raft group per namespace; schemas do not live in the cluster log (FR-003/004) | PASS |
| X | Observability as a Product Surface | Emits `08` leader-election totals/errors/duration; shared-datatype aggregates on the **namespace primary**; names stay in `08` | PASS |
| XI | Documented, Expandable Configuration | `cluster.raft { }` and exclusive-data documented with starters | PASS |
| XII | Raft Controller Elections and Local Restore | **This feature is the principle.** Cluster+namespace Raft; leases not per-write; restore split via `13` | PASS |
| XIII | Security Defaults for Data and Roles | Role **records** persist in the cluster store (blob). Names/verbs remain `07`/`14`. Internode auth stays `12`/`14` | PASS |
| Arch. Contracts | Five-level stack; adapter ≠ type system | Control plane is not a datatype. Handlers still go `002` → `exec` → coordinator | PASS |
| Observability Contract | No `08` series renamed | Additive labels `raft_group`, `raft_role` on election series only if `08` allows additions; required names unchanged | PASS |

**Gate result (pre-research)**: PASS. Sequencing vs `016` (Raft pulled into first binary for cluster + existing namespaces) is `FR-020` and is recorded in Complexity Tracking as an amendment of slice 7's **remaining** surface, not a cancelled MUST.

## Project Structure

### Documentation (this feature)

```text
specs/006-control-plane/
├── plan.md                         # This file
├── research.md                     # Phase 0: R1–R18
├── data-model.md                   # Phase 1: groups, voters, stores, lease, restore
├── quickstart.md                   # Phase 1: 1-node + 3-node election, restore, minority
├── contracts/
│   ├── raft-rpc.md                 # internodes Raft messages (FR-014)
│   ├── cluster-store.md            # ClusterStore impl; cluster log contents (FR-003, FR-012)
│   ├── namespace-store.md          # per-namespace group (FR-004, FR-005)
│   ├── voter-set.md                # odd-size, static first-binary, migrate slice 7
│   ├── leadership-lease.md         # epoch fence (FR-006); slice when ordered types exist
│   ├── restore.md                  # boot orchestration (FR-009, FR-010)
│   ├── metadata-reads.md           # secondary reads, forward/redirect writes (FR-011)
│   ├── metrics-aggregation.md      # namespace primary in-memory merge (FR-013)
│   ├── config-directives.md        # cluster.raft { }; exclusive-data
│   ├── admin-cli.md                # describe controllers, migrate, exclusive-data
│   ├── metrics.md                  # 08 election figures this crate increments
│   └── fixtures/
│       ├── README.md
│       ├── raft-block.conf
│       └── invalid/
├── checklists/requirements.md
└── tasks.md                        # Phase 2 (/speckit-tasks — NOT created here)
```

### Source Code (repository root)

```text
crates/
├── controlplane/                            # spacestorage-controlplane — CORE
│   └── src/
│       ├── lib.rs                           # ClusterStore impl; ControlPlane
│       ├── error.rs                         # NotLeader, Minority, VoterSetOdd, StaleEpoch, NotMember
│       ├── group.rs                         # GroupId { Cluster | Namespace(id) }
│       ├── membership.rs                    # VoterSet, LearnerSet, odd-size checks, joint replace
│       ├── raft_net.rs                      # openraft RaftNetwork → internodes
│       ├── raft_store.rs                    # log + snapshot on disk; fsync spawn_blocking
│       ├── apply.rs                         # committed entries → catalog / ClusterStore events
│       ├── cluster.rs                       # cluster state machine (members, ns list, nodes, drives, roles)
│       ├── namespace.rs                     # schema/definition/shared-meta apply
│       ├── lease.rs                         # LeadershipLease { epoch, holder }; grant/revoke
│       ├── restore.rs                       # boot: load raft → invoke 013; memory empty
│       ├── metrics_agg.rs                   # in-memory merge on namespace primary
│       ├── read.rs                          # read-index / applied reads on followers
│       └── ops.rs                           # slice 7: migrate voter, exclusive-data flag
│
├── internode/                               # additive msg types: RaftVote, RaftAppend, RaftSnapshot,
│                                            # RaftForward, LeaseCheck, MetricsPush
├── placement/                               # ClusterStore trait implemented by controlplane;
│                                            # exclusive-data → not a tenant replica target
├── catalog/ | types/                        # consume applied definitions (003)
├── durability/                              # 013 restore entrypoint invoked from restore.rs
├── config/                                  # cluster.raft { election_timeout; heartbeat; }
│                                            # controller_exclusive_data (slice 7)
├── node/                                    # wire ControlPlane before ready; admin controllers
├── admin-proto/                             # ControllerView, VoterSet, LeaseView, NotLeader
├── spacestorage/                            # CLI: controllers, raft-status, voter-migrate
├── release-profile/                         # first binary: controlplane on; controlplane-ops slice 7
└── conformance/                             # 1/3-node election, minority, restore, 5-node non-voter
```

**Structure Decision**: New `controlplane` crate so `placement` does not take an `openraft` dependency (same reason internodes stayed out of `003`). `node` selects `controlplane::RaftClusterStore` instead of the interim log shipper when internodes is enabled (always, per `012`). First-binary single-node still runs Raft with voter set size 1.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| `016` slice 7 said “control plane Raft”; `FR-020` puts cluster+namespace Raft in slices 1–5 | Membership and schemas are already CP in the first binary (`11`/`03`); an LWW catalog cannot refuse a minority join | Leaving the `004` shipper until slice 7 would make SC-003 false in the first binary. Slice 7 keeps voter **migration**, exclusive-data, quotas, full authz |
| `openraft` instead of a hand-rolled election | Production Raft (joint config, snapshots) is easy to get wrong | Homemade majority vote would miss log matching and snapshot install; etcd sidecar violates III/I |
| Learners in addition to voters | Data nodes need committed definitions without voting | Replicating definitions only to voters would leave non-voters unable to restore local catalogs after isolation |

## Constitution Check (post-design)

Re-evaluated after Phase 1: Raft stays in-process on `internode`; data path remains leaderless; per-namespace groups isolate tenant schema; restore split calls `013` and does not promise RAM; election series names stay in `08`; exclusive-data is a `04` exclusion, not a second placement planner. `016` amendment is tracked above, not a cancelled constitution MUST. **Gate result: PASS.**
