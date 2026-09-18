# Implementation Plan: Cluster Identity, Discovery, Join, Leave, and Replace

**Branch**: `011-identity-membership` | **Date**: 2026-09-18 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/011-identity-membership/spec.md` (Clarifications, Sessions 2026-09-15 and 2026-09-18 — name is a label; secret ≠ membership; restart from persisted membership; replace after failure-detector timeout; operator drain stays up; name reusable / identity retired; pending join or one-time token)

## Summary

Own the **procedures** that produce cluster identity and the membership view `006` stores and `004` consumes: explicit bootstrap (UUID + join secret), seed discovery for first join, pending join vs one-time token, operator drain/undrain, decommission (copy off a still-running node, then retire the identity), and replace of a failure-detector-unavailable identity (incarnation fence). Internode framing, heartbeats, and the replication role stay `012`; Raft internals stay `006`; rebalance algorithms stay `004`. This crate writes **cluster-scoped** `ClusterStore` events and keeps a local identity directory so an already-admitted member can become `ready` without seeds or a new admit.

**First binary** (`016` slices 1–5): one-node bootstrap and three-node join (secret + admit **or** token), rolling restart without re-admit, operator drain + live decommission, replace after heartbeat timeout. Voter-set arithmetic on join/leave follows `006` R4 (1 voter → third member expands to 3; further members are learners). Decommission of a voter in the 3-node first binary leaves **one voter + one learner** (odd size preserved) until a third member is admitted again.

## Technical Context

**Language/Version**: Rust 1.87 (edition 2024, MSRV 1.85), same Cargo workspace as `001`–`016`.

**Primary Dependencies**: existing workspace crates (`tokio`, `async-trait`, `serde`/`serde_json`, `bytes`, `tracing`, `uuid`, `parking_lot`, `rand`/`getrandom` already used for ids). No new Raft library (that is `006`/`openraft`). Join-secret compare is constant-time (`subtle`). Internode frames stay the `004`/`012` length-prefixed envelope. Admin ops extend `001` `admin-proto`. Ladder checks call `004` placement APIs. Fsync of identity files via `spawn_blocking` (`013` pool / constitution II).

**Storage**: `{data_dir}/identity/` — `node.json` (stable node id + name), `cluster.json` (cluster UUID, name label, secret epochs). Cluster membership, pending joins, tokens, retired ids, and incarnation live in the **cluster** `ClusterStore` / Raft group (`004` seam, `006` impl). Not tenant WAL.

**Testing**: `cargo test`. Unit tests in `membership` (secret overlap, token TTL/single-use, name uniqueness, retired-id refuse, incarnation fence, drain vs stop). `crates/conformance` in-process 1/3-node harness: bootstrap+isolated second bootstrap (SC-001/002), pending vs token (SC-003/004/009), drain/undrain/decommission (SC-005), replace after `012` timeout (SC-006), restart without seeds (SC-007), name reuse / identity retire (SC-008). Contract tests on `contracts/fixtures/`.

**Target Platform**: Linux x86_64/aarch64 servers (primary), macOS for development. Multi-node tests: in-process nodes, loopback internodes, same harness as `004`/`006`/`016`.

**Project Type**: Cargo workspace extension — **one new library crate** `crates/membership` (`spacestorage-membership`). No new binary. Wired from `node` before `ready`. Additive internodes message types and admin/CLI ops.

**Performance Goals**: bootstrap to one-member `ready` within the `001` ready bound (SC-001 target < 10 s, same as runtime); admit/token membership visible on every live member < 50 ms p95 on 3-voter loopback after `006` commit (same order as cluster metadata write); pending join is local+leader append only (no tenant-path cost); leaderless KV p95 unchanged (join does not move replicas).

**Constraints**: Membership mutations only via cluster majority (`006` FR-007). Non-members and pending joins do not vote and are not replica targets. Join secret required before cluster protocol (`012`). Every join presents the cluster topology ladder (`004`). Operator drain does not exit; stop signal still exits (`001`). Replace only when `012` has marked the identity unavailable. Fenced incarnation refused on `internode`/`replication`. No merge of two bootstrapped UUIDs. No second client protocol. First binary MUST include bootstrap/join (slice 5).

**Scale/Scope**: First binary 1- and 3-node topologies (`016`); complete product allows more members as learners until slice 7 voter ops. ~10 internodes membership message types; identity dir; pending/token/secret-rotate; drain/decommission/replace orchestration. Roughly: identity+secret (~1.5 k), join/pending/token (~2.5 k), drain/decommission/replace (~2.5 k), admin/config (~1.5 k), conformance (~3 k); ≈ 11–15 k lines including tests.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Gate | Status |
|---|-----------|------|--------|
| I | Rust-Only Stack | New crate and deps are Rust; `uuid`/`subtle`/`getrandom`; no C secret store | PASS |
| II | Fully Asynchronous Tokio Runtime | Join/admit/drain are `async`; identity fsync on `spawn_blocking` | PASS |
| III | Single-Process Multithreaded Monolith | `membership` is a library in `spacestoraged`; no membership sidecar | PASS |
| IV | Type-Driven Multiparadigm | Does not add types. Membership is control data, not a datatype | PASS |
| V | Protocol Compatibility on Distinct Ports | No new client port. Handshake on existing `internode`; admin on `admin` / `admin-http` | PASS |
| VI | Every Node Is a Request Coordinator | Every **member** (not pending) MAY receive client requests (FR-014) | PASS |
| VII | Label-Based Planetary Placement | Join MUST present every key on **this cluster's** topology ladder (`004`); omit/integrity → refuse | PASS |
| VIII | Cassandra-Style Quorum with Protocol Defaults | Unchanged. Drain/decommission only exclude new placements; existing replicas count until rebalance | PASS |
| IX | Multi-Tenant Namespaces | No namespace objects here. Cluster UUID is not a tenant id | PASS |
| X | Observability as a Product Surface | Increments membership counters with names reserved for `08`; does not rename `08` series | PASS |
| XI | Documented, Expandable Configuration | Extends `004` `cluster { }` with bootstrap/join/seeds/secret; starters in fixtures | PASS |
| XII | Raft Controller Elections and Local Restore | **Consumes** `006`: records membership in the cluster log before vote/replica-target. Does not run a second Raft | PASS |
| XIII | Security Defaults for Data and Roles | Join secret + `CLUSTER_ADMIN` admit/token. First binary: interim admin bearer is `CLUSTER_ADMIN` (same `001`/`014` sequencing). Audit hooks for admit/token/replace (`014`) | PASS with justified deviation |
| Arch. Contracts | Five-level stack; adapter ≠ type system | Membership is not a handler type system. Tenant handlers still `002` → exec | PASS |
| Observability Contract | No `08` series renamed | Additive membership figures only | PASS |

**Gate result (pre-research)**: PASS. Sequenced deviations (interim `CLUSTER_ADMIN` = admin token; `004` `token_file` = join secret; voter shrink on 3→2 decommission) are in Complexity Tracking, not cancelled MUSTs.

## Project Structure

### Documentation (this feature)

```text
specs/011-identity-membership/
├── plan.md                         # This file
├── research.md                     # Phase 0: R1–R16
├── data-model.md                   # Phase 1: identity, pending, token, membership, drain
├── quickstart.md                   # Phase 1: bootstrap, join, drain, replace, restart
├── contracts/
│   ├── config-directives.md        # cluster bootstrap/join/seeds/secret (extends 004)
│   ├── internodes-membership.md    # join handshake, fence, secret rotate
│   ├── membership-store.md         # ClusterStore events; pending ≠ members
│   ├── join-secret.md              # generate, overlap rotate, verify
│   ├── drain-decommission-replace.md
│   ├── admin-cli.md                # admit, token, drain, undrain, decommission, replace
│   ├── metrics.md                  # 08 figures this crate increments
│   └── fixtures/
│       ├── README.md
│       ├── bootstrap.conf
│       ├── join-pending.conf
│       ├── join-token.conf
│       └── invalid/
├── checklists/requirements.md
└── tasks.md                        # Phase 2 (/speckit-tasks — NOT created here)
```

### Source Code (repository root)

```text
crates/
├── membership/                              # spacestorage-membership — CORE
│   └── src/
│       ├── lib.rs                           # MembershipService; boot hook
│       ├── error.rs                         # NotMember, Pending, LiveReplace, RetiredIdentity, …
│       ├── identity.rs                      # node.json / cluster.json load-or-create
│       ├── secret.rs                        # epochs, overlap, constant-time verify
│       ├── bootstrap.rs                     # explicit bootstrap; refuse foreign seeds
│       ├── join.rs                          # first-join handshake; pending vs token
│       ├── token.rs                         # mint, bind, TTL, single-use
│       ├── drain.rs                         # operator drain / undrain; not stop
│       ├── decommission.rs                  # wait 004 rebalance; retire identity
│       ├── replace.rs                       # FD-unavailable + incarnation fence
│       ├── events.rs                        # ClusterStore membership records
│       └── metrics.rs                       # counters reserved for 08
│
├── internode/                               # additive: JoinRequest, JoinAck, FenceIncarnation,
│                                            # SecretRotate, PendingAnnounce
├── placement/                               # draining / not-member → not a new replica target;
│                                            # decommission calls rebalance plan
├── controlplane/                            # cluster log apply: members, pending, retired, incarnation
│                                            # (006 MemberRecord.status loses unused `joining`)
├── config/                                  # cluster { bootstrap; join; seeds; secret_file; … }
├── node/                                    # identity before ready; operator drain ≠ stop
├── admin-proto/                             # Admit, JoinToken, Drain, Undrain, Decommission, Replace
├── spacestorage/                            # CLI verbs below
├── release-profile/                         # first binary: membership on
└── conformance/                             # 1/3-node bootstrap, join, drain, replace, restart
```

**Structure Decision**: New `membership` crate so `controlplane` does not own join procedures and `placement` does not own secrets. `node` calls `MembershipService::on_start` before advertising `ready`. `006` remains the store; this crate is the only writer of membership/pending/token/retire/replace events.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| Interim `CLUSTER_ADMIN` = `001` admin bearer until `014` | Admit/token/replace/decommission need an admin principal in slices 1–5 | Blocking membership on full RBAC would slip join out of the first binary (`016` slice 5) |
| `004` `cluster.token_file` is the join secret | `004` already required a cluster token to speak internodes | A second secret file would split `012` “join secret required before cluster protocol” |
| 3-node voter decommission → 1 voter + 1 learner | `006` forbids an even voter set; `011` requires decommission in the three-node test | Waiting for slice 7 voter migrate would make SC-005 false in the first binary |
| Operator drain ≠ `001` stop drain | Live decommission must copy replicas off a running process | Making drain always exit would force data-loss accept for RF=1 |

## Constitution Check (post-design)

Re-evaluated after Phase 1: procedures stay in-process; Raft still only in `006`; ladder enforcement delegates to `004`; pending is not a member so Principle VI applies to members only; operator drain is a membership+lifecycle state, stop still `001`; secrets are cluster credentials not tenant keys. **Gate result: PASS.**
