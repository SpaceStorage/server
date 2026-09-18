# Implementation Plan: Internode Fabric, Clocks, Quorum Domain, and Conflict Resolution

**Branch**: `012-internode-and-time` | **Date**: 2026-09-18 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/012-internode-and-time/spec.md` (Clarifications, Sessions 2026-09-15 and 2026-09-18 — `planet` is never a domain; one source domain; `LOCAL_ONE` write = source WAL; one named domain per member; cluster-wide heartbeat timeout; ordinary vs force promote; follower writes fail with session/query `LOCAL_ONE` fallback; no live domain change)

## Summary

Own the **cluster fabric** that `004` sketched as a single `internode` handler and that `011`/`006` already consume: two always-on entrypoints (`internode`, `replication`), versioned frames, explicit `tls`/`plaintext`, replication-role auth (join secret), a first-class **`quorum_domain`**, one **HLC per domain**, leaderless writes **in the source domain**, async **source-log** apply on followers, LWW-by-HLC in-domain, cluster-wide failure detection, manual promote with epoch fence, and `multi_active=on` refused in the first binary.

`004` still owns replica **targets** (RF, labels, anti-affinity, repair **policy**/rate, hinted-handoff **window**). This feature owns **who counts** for a write/read level, the streams those repairs use, and the clock. `011` join **names** an existing domain; this crate is the only writer of domain objects. `006` Raft RPCs stay on `internode`. Durable ack meaning stays `013`.

**First binary** (`016` slices 1–5): both handlers listening on loopback (and on a cluster address for three-node); one `default` domain; leaderless KV/SQL/document at write `TWO` / read `ONE` inside that domain; heartbeat timeout 15 s; `multi_active=on` create refused. Two-domain promote/fallback is specified and tested in-process; planetary production examples are not required.

## Technical Context

**Language/Version**: Rust 1.87 (edition 2024, MSRV 1.85), same Cargo workspace as `001`–`016`.

**Primary Dependencies**: existing workspace crates (`tokio`, `async-trait`, `serde`/`serde_json`, `bytes`, `tracing`, `uuid`, `parking_lot`). No gRPC, no protobuf-c, no `*-sys`. Frame = `004` length-prefixed envelope (`u32le` length | `u8` version | `u16le` msg_type | payload). HLC is in-house (`crates/clocks`). Join-secret verify is `011` (`subtle`). TLS via `001` cert-path on the entrypoint. Fsync of source-log tails via `spawn_blocking` (`013` pool / constitution II).

**Storage**: Source log and replica streams are tenant data in `003` storage (`ns/<namespace>/<container-id>/` + WAL). Domain objects, container source/epoch, and `multi_active` live in the **cluster** `ClusterStore` (`004` seam, `006` Raft). HLC last-tick is process-local (memory + optional `{data_dir}/clocks/<domain>.json` so a restart does not go backwards). Not a second user-data format.

**Testing**: `cargo test`. Unit tests in `clocks` (HLC order, no cross-domain compare), `internode` (frame version window, backpressure, FD timeout), `replication` (source-log apply, fence). `crates/conformance` in-process 1/3-node + two-domain harness: always-on listeners (SC-001), source-only write acks (SC-002/003/004), fallback (SC-003), `multi_active` refuse (SC-005), LWW vs source log (SC-006), promote/force (SC-007), domain membership (SC-008), FD/replace (SC-009). Contract tests on `contracts/fixtures/`.

**Target Platform**: Linux x86_64/aarch64 servers (primary), macOS for development. Multi-node tests: in-process nodes, loopback internodes/replication, same harness as `004`/`006`/`011`/`016`. Latency injection: existing `internode { delay { … } }` test directive (`SPACESTORAGE_TEST=1`).

**Project Type**: Cargo workspace extension — **two library crates** plus ownership of `crates/internode`: new `crates/clocks` (`spacestorage-clocks`), new `crates/replication` (`spacestorage-replication`). No new binary. Wired from `node` before `ready`. Additive internodes types, admin/CLI, config.

**Performance Goals**: internodes heartbeat RTT < 5 ms p95 on loopback (`004`); HLC tick + stamp assign < 1 µs; source-domain write at `TWO` on 3-replica loopback does not include a follower RTT (SC-010 in `004`); FD marks unavailable within `failure_timeout` + 1 s in 100% of tests; per-stream backpressure never blocks a Tokio worker.

**Constraints**: Both handlers always listen; default bind loopback; cluster (non-loopback) address required to join remotes. Transport `tls` or `plaintext;` required. Peers outside internodes version N/N+1 refused. HLC MUST NOT be compared across domains. Writes in a follower forward to the source; no follower-local WAL. Failure timeout is **cluster-wide** (not per-observer, not phi). Promote is manual. Data path leaderless in the source domain; controllers CP (`006`). No second client protocol. First binary MUST include internodes + replication + one `quorum_domain` (slice 5).

**Scale/Scope**: 2 handlers; ~20 internodes message types (existing `004`/`006`/`011` plus domain/HLC/promote); 1 HLC per domain per node; source log per container; first binary 1- and 3-node one domain. Roughly: clocks (~1 k), internodes completion (~3 k), replication streams (~3 k), domain/promote/quorum count (~2.5 k), config/admin (~1.5 k), conformance (~3 k); ≈ 14–18 k lines including tests.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Gate | Status |
|---|-----------|------|--------|
| I | Rust-Only Stack | In-house HLC and frames; no gRPC/`*-sys` | PASS |
| II | Fully Asynchronous Tokio Runtime | Internode/replication fully `async`; WAL/source-log fsync on `spawn_blocking`; backpressure MUST NOT block workers | PASS |
| III | Single-Process Multithreaded Monolith | Libraries in `spacestoraged`; no fabric sidecar | PASS |
| IV | Type-Driven Multiparadigm | No new datatype hierarchy. Conflict merge comes from the type descriptor (`003`/`004`). `multi_active` is a catalog flag | PASS |
| V | Protocol Compatibility on Distinct Ports | `internode` and `replication` are **internal** handlers on their own ports, not client protocols. Must not share a port with `admin` / `admin-http` or a tenant handler | PASS |
| VI | Every Node Is a Request Coordinator | Every **member** coordinates. Follower-domain coordinators **forward writes to the source**; they still accept client connections | PASS |
| VII | Label-Based Planetary Placement | `planet` / ladder keys NEVER become a domain. `quorum_domain` is a first-class object. RTT + HLC skew published so `005`/`004` can override ladder rank | PASS |
| VIII | Cassandra-Style Quorum with Protocol Defaults | Write `TWO`/`QUORUM` count only durable **source-domain** replicas. `LOCAL_*` is the coordinator's domain. Session/query fallback to `LOCAL_ONE` still a source WAL | PASS |
| IX | Multi-Tenant Namespaces | Fabric is cluster-scoped. Replication metrics labelled with namespace when the stream is per container | PASS |
| X | Observability as a Product Surface | Increments `08` replication + node_state; additive internodes/HLC/FD series; no `08` rename | PASS |
| XI | Documented, Expandable Configuration | Extends `cluster { }` with domain + FD knobs; starters declare both handlers | PASS |
| XII | Raft Controller Elections and Local Restore | Raft stays on `internode` (`006`). Data path leaderless in the source. Ordered types request leadership (`006`); LWW MUST NOT repair two ordered histories | PASS |
| XIII | Security Defaults for Data and Roles | Replication-role auth + join secret before cluster protocol. First binary: secret verify is the replication principal until `014` | PASS with justified deviation |
| Arch. Contracts | Five-level stack; adapter ≠ type system | Fabric is L1 transport, not a handler type system. Tenant drivers still `002` → exec | PASS |
| Observability Contract | No `08` series renamed | Additive internodes/HLC/domain figures only | PASS |

**Gate result (pre-research)**: PASS. Sequenced deviations (always-on handlers vs `004` enable/disable; `quorum_domain` vs `004` `locality_key`; cluster-wide FD vs per-group timeout; interim replication role = join secret) are in Complexity Tracking.

## Project Structure

### Documentation (this feature)

```text
specs/012-internode-and-time/
├── plan.md
├── research.md                     # Phase 0: R1–R16
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── handlers.md                 # always-on internodes + replication
│   ├── frame.md                    # version window, backpressure
│   ├── quorum-domain.md
│   ├── clocks.md                   # HLC, skew
│   ├── failure-detector.md
│   ├── source-follower.md          # write/ack/read/fallback
│   ├── promote.md
│   ├── replication-streams.md
│   ├── config-directives.md
│   ├── admin-cli.md
│   ├── metrics.md
│   └── fixtures/
│       ├── README.md
│       ├── single-node.conf
│       ├── three-node-a.conf
│       ├── two-domain-source.conf
│       └── invalid/
├── checklists/requirements.md
└── tasks.md                        # Phase 2 (/speckit-tasks — NOT created here)
```

### Source Code (repository root)

```text
crates/
├── clocks/                                  # spacestorage-clocks — NEW
│   └── src/
│       ├── lib.rs                           # Hlc, DomainClock, compare in-domain only
│       ├── stamp.rs                         # (physical_micros, logical, node_id)
│       └── skew.rs                          # samples, max_stamp_skew, health
│
├── internode/                               # spacestorage-internode — OWNED here (started in 004)
│   └── src/
│       ├── lib.rs                           # InternodeService; always listen
│       ├── frame.rs                         # length-prefixed; N/N+1
│       ├── auth.rs                          # join secret; replication role
│       ├── heartbeat.rs                     # cluster-wide FD
│       ├── rtt.rs                           # EMA for 004/005 rank override
│       ├── backpressure.rs                  # per-stream; never block workers
│       ├── fanout.rs                        # in-domain FanoutWrite/Read/Ack
│       └── registry.rs                      # msg types (004/006/011 + 012)
│
├── replication/                             # spacestorage-replication — NEW
│   └── src/
│       ├── lib.rs                           # ReplicationService; always listen
│       ├── source_log.rs                    # per-container source sequence
│       ├── follower.rs                      # apply source log; no independent log
│       ├── stream.rs                        # bulk copy, repair bytes, hints
│       └── fence.rs                         # epoch; refuse old-source writes
│
├── placement/                               # who counts: source vs follower; RTT override;
│                                            # stamp.rs becomes a clocks re-export
├── membership/                              # JoinRequest.quorum_domain required
├── controlplane/                            # domain create/promote events on cluster log
├── config/                                  # internodes/replication entrypoints; cluster.failure_timeout
├── node/                                    # both handlers before ready; loopback default
├── admin-proto/                             # DomainCreate, Promote, ForcePromote
├── exec/                                    # forward writes from follower; fallback LOCAL_ONE
├── types/                                   # multi_active catalog flag; create-on refuse
├── release-profile/                         # first binary: internodes+replication on
└── conformance/                             # SC-001–SC-009
```

**Structure Decision**: `clocks` has no TCP so `006`/`003` can stamp metadata without taking internodes. `internode` stays the control/coordination port (`004`/`006`/`011` RPCs + in-domain fan-out). `replication` owns **bytes of container data** (source log, bootstrap/repair streams). `placement` keeps RF and repair **policy**; it calls these crates for streams and for “does this replica count?”.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| Always-on `internode`/`replication` (no `disable internodes;`) | Spec FR-002; single-node and join both need listeners | `004` enable-or-disable would let a node come `ready` with no cluster port |
| `quorum_domain` replaces `004` `cluster.locality_key` as the voting/HLC set | Constitution VII / clarify: labels MUST NOT silently become a domain | Keeping `locality_key region` would make `planet`/`region` a voting set |
| Cluster-wide `failure_timeout` only (no per-group FD) | Replace eligibility (`011`) and quorum unavailability must agree across observers | `004` per-group timeout would make replace race on a slow link |
| Interim replication role = join secret (`011` `token_file`) until `014` | First binary must refuse unauthenticated peers (slice 5) | Blocking fabric on full RBAC would slip internodes out of the first binary |
| HLC moves from `placement/stamp.rs` to `crates/clocks` | `006` metadata stamps and data stamps share one clock per domain | Leaving HLC in `placement` would make controlplane depend on the planner |
| Bulk repair/bootstrap bytes on `replication`, not `internode` | Spec splits streaming copy from coordination | Keeping RepairChunk on internodes would mix bulk copy with Raft/heartbeats |

## Constitution Check (post-design)

Re-evaluated after Phase 1: two internal handlers on distinct ports; HLC isolated per domain; data path still leaderless in the source; Raft still only on `internode`; follower writes never take a local WAL; FD is one cluster timeout; metrics add series only. **Gate result: PASS.**
