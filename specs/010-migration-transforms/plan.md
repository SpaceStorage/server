# Implementation Plan: Data Migration and Type/Model Transforms

**Branch**: `010-migration-transforms` | **Date**: 2026-09-18 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/010-migration-transforms/spec.md` (Clarifications, Session 2026-09-18 — five rewrite kinds; live dual-write transforms; refuse existing destination names; move drops source; quota at start and cutover; catalog mapping vs mapping query)

## Summary

Add an in-process **job runner** that owns four `08` job names — `data_migration`, `data_transformation`, `data_backup`, `data_restore` — without a sidecar and without a second snapshot format.

**Migration** is an operator/tenant-initiated copy or move (nodes/drives/namespaces), default **live copy + dual-write + cutover**. Automatic RF/anti-affinity repair stays `04`. **Transform** is a background rewrite into a **new** container (type/model, incompatible schema, re-encode/recompress, re-encrypt, sharding-key) with optional name swap. Understandable types use a **catalog mapping**; complex sources require a **mapping query** (named source containers/columns → destinations) evaluated through `05`. **Backup/restore jobs** schedule policy and invoke `13` snapshot/PITR.

**First binary** (`016` slices 1–5): no migrate/transform/backup jobs (`MigrateSlice10Required` if configured). **Slice 10**: this plan.

## Technical Context

**Language/Version**: Rust 1.87 (edition 2024, MSRV 1.85), same Cargo workspace as `001`–`016`.

**Primary Dependencies**: existing workspace only (`tokio`, `serde`/`serde_json`, `uuid`, `bytes`, `tracing`, `async-trait`, `parking_lot`). Dual-write intercepts type ops (`003`) and placement (`004`). Mapping query lowers to `005` `LogicalRequest` (structured AST, not a second SQL engine). Snapshots via `013` APIs. Authz `MIGRATE` / `CLUSTER_ADMIN` (`014`). Job series names frozen in `008`. No `*-sys`, no dump/reload tools.

**Storage**: Job records in **controller storage** (`006`): cluster log for cluster-scoped jobs (node evacuate, cross-namespace, backup of several namespaces); namespace Raft for in-namespace transform/copy. Incomplete targets are real containers with `incomplete` on the description, hidden from tenant list until swap/cutover. No second WAL format; catch-up reads source through type ops; dual-write applies mapped writes through the same ops. Snapshot bytes remain `013`.

**Testing**: `cargo test`. Unit: mapping catalog vs query refuse, name collision, quota start/cutover, dual-write sequence, move drop. `crates/conformance` (feature `migrate`): two- then three-node live migrate; namespace copy/move; five rewrite kinds; live writes through swap; mapping query; missing mapping query; snapshot+restore job; encrypted backup missing key. Contract tests on `contracts/fixtures/`.

**Target Platform**: Linux x86_64/aarch64 servers (primary), macOS for development. Multi-node: in-process loopback, same harness as `004`/`006`.

**Project Type**: Cargo workspace extension — **one library crate** `crates/migrate` (`spacestorage-migrate`). No new binary. Optional compile on slice 10 (`release-profile`).

**Performance Goals**: SC-006 — operator following [quickstart.md](quickstart.md) migrates a small KV container off a node in **under 20 minutes**. Dual-write MUST NOT ack a source write the target cannot take. Catch-up of a small fixture (≤ 10 k objects) completes in the same window. Job progress (bytes/objects, last sequences) is queryable while running. No new point-query SLO; idle dual-write interceptor adds < 5 % p95 on loopback KV vs `004` when no job is active (interceptor unregistered).

**Constraints**: Replica streaming/repair is `04` only. Transforms are never in-place mutation of the live source. Destination name collision → refuse. Move cutover **drops** source. Quota: refuse at start if dest cannot hold a second copy; re-check before cutover/swap. Dual-write failure pauses the job and fails the write named. First binary MUST NOT require jobs. Backup jobs MUST call `13`, not invent snapshots. Mapping-query evaluation MUST use `005`, not a private engine.

**Scale/Scope**: Four job kinds; three migrate strategies (live default, snapshot+restore, offline copy); five transform rewrite kinds; catalog mapping + mapping query; backup/restore orchestration. Roughly: job runner+store (~2 k), dual-write+catch-up (~3 k), transform+mapping (~3 k), backup invoke `13` (~1 k), admin/CLI (~1 k), conformance (~3 k); ≈ 12–15 k lines including tests.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Gate | Status |
|---|-----------|------|--------|
| I | Rust-Only Stack | One Rust crate; no dump tools, no C snapshot agents | PASS |
| II | Fully Asynchronous Tokio Runtime | Job steps are `async`; catch-up scans via type ops; CPU-heavy re-encode/re-encrypt on existing `spawn_blocking` pool; durable acks wait `013` | PASS |
| III | Single-Process Multithreaded Monolith | Runner inside `spacestoraged`; no migrate sidecar | PASS |
| IV | Type-Driven Multiparadigm | Rewrites go through `003` types; catalog mappings live on type descriptors; mapping query is not a second type system | PASS |
| V | Protocol Compatibility on Distinct Ports | No new client protocol. Jobs on admin CLI/HTTP. Clients keep existing ports during live migrate | PASS |
| VI | Every Node Is a Request Coordinator | Dual-write on the coordinating write path; any node may accept writes during the window; job may be driven from any `ready` node | PASS |
| VII | Label-Based Planetary Placement | Cutover validates `04` anti-affinity/labels/drives; unsatisfiable migrate refuses | PASS |
| VIII | Cassandra-Style Quorum | Dual-write source ack still uses container write quorum + durable filter; target apply uses the target's declared quorum | PASS |
| IX | Multi-Tenant Namespaces | Copy/move are namespace-scoped; collision and quota are per destination namespace; incomplete targets are not tenant-visible | PASS |
| X | Observability as a Product Surface | Increment `08` job series with `job=data_*`; progress fields on the job object for `09`; no rename of required labels | PASS |
| XI | Documented, Expandable Configuration | Starter evacuate/copy/transform/backup in fixtures; catalog mappings expandable with types | PASS |
| XII | Raft Controller Elections and Local Restore | Job records in `006` cluster/namespace logs; resume from recorded sequence after node restart; data restore is `013` | PASS |
| XIII | Security Defaults for Data and Roles | `MIGRATE` / admin (`014`); encrypted backups keep key references; re-encrypt transform uses `14` keys | PASS |
| Arch. Contracts | Five-level stack; adapter ≠ type system | Mapping query → `005` IR → type ops. No parallel hierarchy | PASS |
| Observability Contract | Intent labels/series | Additive use of existing job families only | PASS |

**Gate result (pre-research)**: PASS. Slice 10 deferral matches `016`. Default drop-after-swap is a planning default for the spec's remaining "per policy" (Complexity Tracking).

## Project Structure

### Documentation (this feature)

```text
specs/010-migration-transforms/
├── plan.md                         # This file
├── research.md                     # Phase 0: R1–R16
├── data-model.md                   # Phase 1: job, migrate, transform, mapping, backup
├── quickstart.md                   # Phase 1: evacuate, copy/move, transform, backup
├── contracts/
│   ├── jobs.md                     # job object, status, progress, resume, cancel
│   ├── migrate.md                  # node/drive/namespace, strategies, move drop
│   ├── transform.md                # five rewrite kinds, swap, incomplete target
│   ├── mapping.md                  # catalog mapping vs mapping query AST
│   ├── dual-write.md               # interceptor, sequences, cutover/swap gates
│   ├── backup-restore.md           # jobs invoke 13; no second format
│   ├── admin-http.md               # /v1/jobs, /v1/migrate, /v1/transform, /v1/backup
│   ├── cli.md                      # spacestorage migrate | transform | backup | restore | job
│   ├── metrics.md                  # 08 job=data_* increments
│   ├── config-directives.md        # jobs { }; first-binary refuse
│   └── fixtures/
│       ├── README.md
│       ├── evacuate-live.json
│       ├── namespace-copy.json
│       ├── transform-catalog.json
│       ├── transform-mapping-query.json
│       ├── backup-namespace.json
│       └── invalid/
├── checklists/requirements.md
└── tasks.md                        # Phase 2 (/speckit-tasks — NOT created here)
```

### Source Code (repository root)

```text
crates/
├── migrate/                                 # spacestorage-migrate — CORE (slice 10)
│   └── src/
│       ├── lib.rs                           # JobService, start/cancel/status
│       ├── job.rs                           # JobId, JobKind, JobStatus, progress
│       ├── store.rs                         # ClusterStore / namespace log records
│       ├── authz.rs                         # MIGRATE, CLUSTER_ADMIN, dual grant
│       ├── quota.rs                         # start + cutover checks via 07
│       ├── dual_write.rs                    # interceptor, source seq, catch-up
│       ├── migrate.rs                       # live / snapshot / offline; node+namespace
│       ├── transform.rs                     # five rewrite kinds, swap, drop-or-retain
│       ├── mapping.rs                       # CatalogMapping + MappingQuery → LogicalRequest
│       ├── backup.rs                        # DataBackup / DataRestore → 13
│       └── error.rs                         # named refuses (quota, name, mapping, constraint)
│
├── types/                                   # catalog default_transform; incomplete flag
├── exec/                                    # execute mapping-query LogicalRequest
├── placement/                               # validate dest before cutover (seam)
├── storage/                                 # 13 SnapshotApi consumed, not reimplemented
├── controlplane/                            # persist JobRecord
├── node/                                    # register interceptor + job supervisor
├── admin-proto/ | spacestorage/             # CLI + JSON types
├── release-profile/                         # first binary: stub; slice 10: full
└── conformance/                             # live migrate, copy/move, transforms, backup
```

**Structure Decision**: One crate so job lifecycle, dual-write, and transform share one resume state. Backup is a thin module over `13`. UI progress stays `09` (reads job progress). Compaction/TTL jobs stay `13`.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| Dual-write on the live write path | Spec: source writable; swap includes every durable ack; MUST NOT ack a source write the target cannot take | Freeze writers (rejected in clarify); async catch-up only (can lose acks at swap) |
| Mapping query AST instead of free SQL | Complex types need named sources/destinations; `05` already owns SQL | Embed a second SQL parser (constitution IV / `005` one IR) |
| Default drop of previous container after transform swap | Spec left retain-vs-drop "per policy"; tests need one default | Always retain (leaks names/quota like the rejected empty move source) |

## Constitution Check (post-design)

Re-evaluated after Phase 1: jobs are Tokio tasks in `spacestoraged`; records in `006`; dual-write is an interceptor on type ops; mapping query is `LogicalRequest`; snapshots only via `13`; `08` job label values already listed; first binary gated by `MigrateSlice10Required`. **Gate result: PASS.**
