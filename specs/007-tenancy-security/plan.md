# Implementation Plan: Namespaces, Quotas, Access Policies, Encryption Attachment, and Roles

**Branch**: `007-tenancy-security` | **Date**: 2026-09-16 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/007-tenancy-security/spec.md` (Clarifications, Session 2026-09-16 — first-binary namespace CRUD+rename + `admin`/`replication` + encryption declarations; cluster store for registry/quotas/policies/bindings; logical quota usage; `CLUSTER_ADMIN`-only namespace and quota writes; best-effort hard reject; renameable namespace names and `14` login names)

## Summary

Fill the **cluster-store tenancy records** that `006` already replicates as a namespace **list** plus an opaque **role blob**. This feature owns the interpreted model: namespace registry (name, quotas, access policies, private-telemetry flags), role names `admin` / `replication` / `custom`, role bindings, and quota **admission** on every coordinator. Schemas and container definitions stay in **namespace Raft** (`06`). Encryption at rest is a **container declaration** (`03` algorithm, key reference, scope); this crate validates that key material never appears in descriptions. AuthN, permission verbs, KEK unwrap, and audit remain `14`.

**First binary** (`16` slices 1–5): create/list/**rename**/delete namespaces (`CLUSTER_ADMIN`), documented starter namespace, persist and honor `admin` and `replication`, accept encryption declarations. Unset quotas MUST NOT reject. **Slice 7** (`tenancy-quotas`): hard quotas (logical bytes/rows, connections, optional ops/s), custom roles, per-type access policies. **Private tenant metrics/logs** wait for `08` (flags may be stored; export is not required). Principal login rename is specified in `14` (bindings here use principal id).

Quota enforcement is a **best-effort hard reject** on the data path: in-flight work MAY briefly exceed; later consuming requests reject; never hang; **no** cluster-primary serialization of tenant writes.

## Technical Context

**Language/Version**: Rust 1.87 (edition 2024, MSRV 1.85), same Cargo workspace as `001`–`016`.

**Primary Dependencies**: existing workspace crates (`tokio`, `async-trait`, `serde`/`serde_json`, `bytes`, `tracing`, `uuid`, `parking_lot`). No new Raft library (uses `006` `ClusterStore`). No KMS client (`14`). Internodes additive `QuotaDelta` for best-effort usage. Config grammar already reserved `namespace` (`001`).

**Storage**: Tenancy **control** records (registry, quotas, policies, bindings) are cluster-log entries via `controlplane` (`{data_dir}/raft/cluster/`). Not tenant WAL. Usage ledgers are **in-memory** (reconciled from catalog logical sizes); they are not Raft-committed. Encryption declarations live on container definitions in namespace Raft (`03`/`06`).

**Testing**: `cargo test`. Unit tests in `tenancy` (name rules, rename unique index, logical vs RF, admit/reject, overshoot, CLUSTER_ADMIN gate). `crates/conformance`: first-binary namespace CRUD+rename, starter `acme`, refuse `NAMESPACE_ADMIN` create/delete/rename, encryption describe with no key material, memory-mode `drives` scope refuse. Feature `tenancy-quotas`: fill-to-cap reject, sibling namespace unaffected, RF does not multiply usage, concurrent overshoot never hangs. Contract tests on `contracts/fixtures/`.

**Target Platform**: Linux x86_64/aarch64 servers (primary), macOS for development. Multi-node tests: in-process, loopback, same harness as `004`/`006`/`016`.

**Project Type**: Cargo workspace extension — **one new library crate** `crates/tenancy` (`spacestorage-tenancy`). No new binary. Serializes cluster-log bodies; `controlplane` remains Raft-only. Admin/CLI and protocol handlers consume this crate. `release-profile`: first binary compiles registry+builtin roles+encryption attach; `tenancy-quotas` is slice 7.

**Performance Goals**: `NamespaceCreate` < 50 ms p95 on 3-voter loopback (bound by cluster Raft, same class as `006` create-namespace). Quota check on a coordinator < 50 µs p99 after caps are cached (atomics; no Raft). Leaderless KV p95 unchanged vs `004`/`006` when quotas unset. List namespaces without contacting namespace Raft (SC-009).

**Constraints**: Only `CLUSTER_ADMIN` mutates registry (create/rename/delete) and quotas. `NAMESPACE_ADMIN` MAY read usage. Logical usage; RF MUST NOT multiply. Best-effort overshoot; never hang; never wait on cluster primary for quota. CPU isolation is a non-goal. Key material NEVER in this feature's logs or describes. First binary MUST NOT require quota reject, custom roles, or private export. UIs (`09`) get no extra rights. Usage ledger and bindings key by UUID so rename does not rewrite them.

**Scale/Scope**: Tens to hundreds of namespaces; a handful of quota rows per namespace (namespace-level + per-type). Builtin two roles plus N custom (slice 7). ~8 internodes-adjacent types (`QuotaDelta`). Roughly: registry+roles (~2 k), admit+usage (~2 k), encryption attach (~0.5 k), admin/config (~1 k), conformance (~2 k); ≈ 7–10 k lines including tests.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Gate | Status |
|---|-----------|------|--------|
| I | Rust-Only Stack | New crate is Rust; no C KMS/etcd | PASS |
| II | Fully Asynchronous Tokio Runtime | Admit path is async, non-blocking; cluster writes already fsync off-worker in `06` | PASS |
| III | Single-Process Multithreaded Monolith | `tenancy` is a library in `spacestoraged`; no sidecar | PASS |
| IV | Type-Driven Multiparadigm | Does not add types. Per-type quotas/policies **select** `03` type names | PASS |
| V | Protocol Compatibility on Distinct Ports | No new client port. PG database / Redis binding consume registry (`02`) | PASS |
| VI | Every Node Is a Request Coordinator | Quota check runs on the coordinating node; does not force cluster primary | PASS |
| VII | Label-Based Planetary Placement | Unchanged. RF is `04`; quotas do not count replicas | PASS |
| VIII | Cassandra-Style Quorum | Data path stays leaderless + quorum. Caps are CP (cluster Raft); usage is AP/best-effort | PASS |
| IX | Multi-Tenant Namespaces | **This feature is the principle.** Namespace = tenant; quotas/policies per type; metrics/logs per tenant vs global (`08` export) | PASS |
| X | Observability as a Product Surface | Tracks usage and `quota_exceeded`; names reserved for `08`; private export waits for `08` | PASS |
| XI | Documented, Expandable Configuration | `starter_namespace` and slice-7 quota directives with starters | PASS |
| XII | Raft Controller Elections and Local Restore | Registry in cluster log; schemas stay in namespace Raft (`06` FR-003/004). Restore of roles/quotas = replay cluster log | PASS |
| XIII | Security Defaults for Data and Roles | Encryption declaration per container; roles in cluster-level controller storage | PASS |
| Arch. Contracts | Five-level stack; adapter ≠ type system | Tenancy is not a datatype. Handlers still `002` → exec → coordinator | PASS |
| Observability Contract | No `08` series renamed | Additive usage/quota-reject series; required names unchanged | PASS |

**Gate result (pre-research)**: PASS. Slice 7 for hard quotas matches `016` and spec FR-013. First-binary namespaces are already required by `006` FR-020 and PG/Redis smoke.

## Project Structure

### Documentation (this feature)

```text
specs/007-tenancy-security/
├── plan.md                         # This file
├── research.md                     # Phase 0: R1–R16
├── data-model.md                   # Phase 1: registry, quota, role, policy, usage
├── quickstart.md                   # Phase 1: CRUD, encryption, slice-7 quotas
├── contracts/
│   ├── cluster-registry.md         # namespace records in cluster log (FR-001, FR-006)
│   ├── roles.md                    # admin / replication / custom; bindings (FR-005, FR-007)
│   ├── quotas.md                   # units, logical, best-effort, CLUSTER_ADMIN (FR-002–003, FR-014)
│   ├── access-policies.md          # per-type policies, slice 7 (FR-004)
│   ├── encryption-declaration.md   # attach on container; never keys (FR-010–011)
│   ├── admission.md                # coordinator check; 005 stricter-wins
│   ├── admin-cli.md                # namespace/quota/role ops
│   ├── config-directives.md        # starter_namespace; quota block slice 7
│   ├── metrics.md                  # 08 figures this crate increments
│   ├── protocol-mapping.md         # PG database / Redis bind consume registry
│   └── fixtures/
│       ├── README.md
│       ├── namespace-block.conf
│       └── invalid/
├── checklists/requirements.md
└── tasks.md                        # Phase 2 (/speckit-tasks — NOT created here)
```

### Source Code (repository root)

```text
crates/
├── tenancy/                                 # spacestorage-tenancy — CORE
│   └── src/
│       ├── lib.rs                           # Tenancy; NamespaceRegistry handle
│       ├── error.rs                         # QuotaExceeded, NamespaceExists, CascadeRequired,
│       │                                    # NotClusterAdmin, KeyMaterialForbidden
│       ├── name.rs                          # uniqueness + charset
│       ├── registry.rs                      # NamespaceRecord serde → ClusterStore; rename index
│       ├── roles.rs                         # Role, RoleBinding; builtin admin/replication
│       ├── quotas.rs                        # QuotaSpec; logical usage; slice 7
│       ├── policy.rs                        # AccessPolicy; slice 7
│       ├── usage.rs                         # in-memory ledger + QuotaDelta apply
│       ├── admit.rs                         # check/increment/decrement; never Raft
│       ├── encryption.rs                    # validate declaration; strip keys from views
│       └── ops.rs                           # slice 7: quota set, custom roles, policies
│
├── controlplane/                            # cluster log bodies include NamespaceRecord
│                                            # (replaces opaque list+blob with typed records)
├── internode/                               # additive QuotaDelta { namespace, unit, delta }
├── catalog/ | types/                        # encryption fields on container definition (`03`)
├── exec/                                    # call tenancy::admit before Scheduled (`05`)
├── node/                                    # session connect/disconnect → connection quota
├── config/                                  # cluster.starter_namespace; namespace.quota (slice 7)
├── admin-proto/                             # NamespaceView, QuotaView, RoleView
├── spacestorage/                            # CLI: namespaces, quotas, roles
├── protocol-pg/ | protocol-redis/           # map database/AUTH to registry (`02`)
├── release-profile/                         # first binary: tenancy on; tenancy-quotas slice 7
└── conformance/                             # CRUD, encryption describe, slice-7 quota suite
```

**Structure Decision**: New `tenancy` crate so `controlplane` stays a Raft adapter (same split as `006` vs `placement`). First-binary single-node still has a cluster group of size 1; creating `acme` writes the registry and starts namespace Raft (`06`).

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| Interpreting `006`'s opaque role blob | Spec requires named roles and bindings in cluster storage | Leaving bytes opaque would leave `admin`/`replication` unimplementable in the first binary (`14` needs a store) |
| In-memory usage + internodes `QuotaDelta` instead of Raft usage | Clarify Q5: never serialize tenant writes on cluster primary | Raft-committed usage would pin every insert to cluster Raft and contradict `06` leaderless data path |

## Constitution Check (post-design)

Re-evaluated after Phase 1: registry/quotas/bindings stay on the cluster log; schemas stay in namespace Raft; quota admit is coordinator-local; encryption is a `03` field with key references only; private export is a stored flag whose implementation is `08`; UIs get the same `14` verbs. **Gate result: PASS.**
