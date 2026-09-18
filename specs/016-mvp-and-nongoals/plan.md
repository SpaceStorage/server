# Implementation Plan: MVP Cut, Sequencing, and Product Non-Goals

**Branch**: `016-mvp-and-nongoals` | **Date**: 2026-09-18 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/016-mvp-and-nongoals/spec.md` (Clarifications: Session 2026-09-15, Session 2026-09-18)

## Summary

Make the complete-product surface in `01`–`15` **implementable without pretending it is one release**. This feature does not replace those specs and does not add a second type hierarchy or a second query engine. It adds a **release profile**, a **slice ledger**, **first-binary starter configs**, and a **conformance profile** so that:

- Implementation follows slices 1 → 11 (runtime → types+durability → PostgreSQL subset → Redis subset → membership/internode/three-node quorum → remaining handlers → Raft/tenancy/authz → query beyond CRUD → observability catalog → migration/backup → UIs/ingest).
- The **first shippable binary** is slices **1–5** only: one-node and three-node topologies with cluster ladder `[az]`; creatable types `K/V Store`, `Relational Table`, `Document Store` (Document Store: admin-create, CRUD as canonical blob over PostgreSQL and/or Redis); PostgreSQL smoke without `COPY`; `BEGIN`/`COMMIT`/`ROLLBACK` all `0A000`; Redis MUST list on `K/V Store` only — type-specific verbs off that list error, never silent success; leaderless replication in an explicit `quorum_domain`; **product default** write `ack==2` / read `ack==1` durable WAL acks; **one-node starter overrides `write_quorum ONE`**, three-node keeps TWO (TWO is not `min(2, live replicas)`); UUID + join secret; always-on `internode`/`replication` (default loopback); every entrypoint `tls` or `plaintext;`; admin CLI/HTTP and drain; global `/metrics` for implemented paths; cluster master-key file; `multi_active=on` create refused. Cassandra / Elasticsearch / ClickHouse / S3 / WebDAV handlers are **absent** from that binary (unknown-handler startup failure).
- Slices 6–11 remain **owed** (deferred in the milestone record, never deleted from intent). Constitution MUSTs this file defers are still owed.
- Product **non-goals** are refused or listed Out of Scope — they are not a silent backlog.

Numeric specify order (`01`…`16`) is unchanged. Implementation of a milestone MAY skip later UI/ingest work if the ledger records it as deferred.

## Technical Context

**Language/Version**: Rust 1.87 (edition 2024, MSRV 1.85), same Cargo workspace as `001`–`015`.

**Primary Dependencies**: existing workspace crates only (`tokio`, `serde`/`serde_json`, `clap`, `tracing`, handler crates from `002`, `config`/`node` from `001`, `types` from `003`, `placement`/`internode` from `004`, `conformance`). No new C-backed crates. No new runtime libraries. This feature adds a **release-profile** module and conformance suites, not a new protocol.

**Storage**: N/A as a new format. First-binary durability is `013` (WAL on drives). Cluster identity/secret and master-key **file paths** are `011`/`014`. Slice ledger and milestone records are repository files (YAML + markdown), not cluster metadata.

**Testing**: `cargo test -p spacestorage-conformance --features first-binary` is the first-binary gate (SC-001–SC-003, SC-006). A separate `--features complete-product` suite is **not** required to pass for a 1–5 milestone. Unit tests in `config` reject unknown handlers and omitted transport. One-node fixture DML uses starter `write_quorum ONE`; three-node kill-one uses product TWO. A ledger check (`scripts/check-milestone.sh` or `cargo test -p spacestorage-release-profile`) fails if a 1–5 changelog omits slices 6–11 as deferred (SC-004). A static review fixture lists product non-goals vs `01`–`15` Out of Scope (SC-005).

**Target Platform**: Linux x86_64/aarch64 servers (primary), macOS for development. Multi-node tests: three in-process nodes on loopback, same harness style as `004`.

**Project Type**: Cargo workspace extension — one small library crate (`release-profile`) plus conformance profiles and starter fixtures. No new binaries. Default `spacestoraged` **build** is the first-binary handler set.

**Performance Goals**: SC-001 — a new operator following [quickstart.md](quickstart.md) brings up the three-node cluster and completes PG + Redis smokes (plus Document Store blob CRUD) in **under 60 minutes**. First-binary conformance (1-node start with write ONE, 3-node join, PG/Redis smoke, Document Store blob, `HGET` error, kill-one, write at TWO, restart+restore) completes in CI in minutes on loopback. No new throughput targets; `004`/`013` numbers stand.

**Constraints**: Do not brand the seven-protocol matrix as "v1". Do not delete or shrink `01`–`15` MUSTs when a milestone skips them. First-binary PostgreSQL: no `COPY`; `BEGIN`/`COMMIT`/`ROLLBACK` all `0A000`. First-binary Redis type-specific verbs off the K/V MUST list MUST error (unknown command or not-supported), never succeed or no-op. Product write quorum stays TWO; one-node starter MUST set `write_quorum ONE`; TWO MUST NOT mean `min(2, live replicas)`. `Document Store` is admin-created; protocol I/O is canonical blob. `multi_active=on` create refused. Handler names for unimplemented protocols fail as **unknown handler** (`001`). Every entrypoint declares `tls` or `plaintext;`. `internode` and `replication` always listen (default loopback). Topology ladder default `[az]`. Catalog `multi_active` default off.

**Scale/Scope**: 11 slices; 5 in the first binary; 8 product non-goals; 2 starter topologies (1-node, 3-node); 2 dialect subsets (PG, Redis); 3 required L3 types; 2 client handlers in the default binary. Roughly 1 crate + conformance profile + fixtures; ~2–4k lines including tests. Sibling features supply the behavior this profile **gates**.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Gate | Status |
|---|-----------|------|--------|
| I | Rust-Only Stack | No new non-Rust deps; fixtures and ledger are YAML/markdown consumed by Rust tests | PASS |
| II | Fully Asynchronous Tokio Runtime | Conformance boots nodes on the existing runtime; this feature adds no blocking path | PASS |
| III | Single-Process Multithreaded Monolith | First binary is still one `spacestoraged` process; omitted handlers are compile-time features, not sidecars | PASS |
| IV | Type-Driven Multiparadigm | First binary **requires** three L3 types; the catalog from `003` remains the inventory. L0 creatable-as-workflow is later, not a parallel hierarchy | PASS |
| V | Protocol Compatibility on Distinct Ports | Complete product still owes all handlers (`15`). First binary ships PostgreSQL + Redis only, which constitution V already defers to this file. Unknown handler for the rest | PASS |
| VI | Every Node Is a Request Coordinator | Three-node first binary: any member receives PG/Redis; no proxy | PASS |
| VII | Label-Based Planetary Placement | Default ladder `[az]`; `planet` remains a legal key, not a voting set; production planetary examples not required | PASS |
| VIII | Cassandra-Style Quorum with Protocol Defaults | Product write `TWO` / read `ONE` inside the source `quorum_domain`; durable WAL acks (`12`/`13`). One-node starter **overrides** write to ONE (FR-009); TWO is not `min(2, live replicas)` | PASS |
| IX | Multi-Tenant Namespaces | Namespaces exist (PG database / Redis binding). Quotas and full policy are slice 7 (deferred, not deleted) | PASS |
| X | Observability as a Product Surface | Global `/metrics` for implemented paths; full `08` catalog is slice 9 (deferred). No `08` series renamed | PASS |
| XI | Documented, Expandable Configuration | Starter one-node and three-node fixtures; ladder and handler inventories stay open | PASS |
| XII | Raft Controller Elections and Local Restore | Raft is slice 7. First binary restores local durable data (`13`) and membership via the `004`/`011` interim store. Tracked below | PASS with justified sequencing |
| XIII | Security Defaults for Data and Roles | First binary: master-key file, explicit transport, SCRAM/AUTH, admin token. Full permission vocabulary is slice 7 (deferred). Tracked below | PASS with justified sequencing |
| Arch. Contracts | Four-level stack; adapter ≠ type system | Release profile selects **which** adapters and types a build **must** include; it does not invent a second stack | PASS |
| Observability Contract | No required `08` label dropped | First binary emits labels that exist for implemented paths; missing families are absent series, not renamed ones | PASS |

**Gate result (pre-research)**: PASS. Sequencing deviations (no Raft, incomplete authz/quotas/metrics catalog/handlers in the first binary) are the **purpose** of this feature and are recorded as deferred slices, not cancelled constitution MUSTs.

## Project Structure

### Documentation (this feature)

```text
specs/016-mvp-and-nongoals/
├── plan.md                         # This file
├── research.md                     # Phase 0: R1–R20
├── data-model.md                   # Phase 1: slice, profile, ledger, non-goal
├── quickstart.md                   # Phase 1: <60 min 1-node then 3-node + smokes
├── contracts/
│   ├── slices.md                   # Slice order, intent map, skip = deferred
│   ├── first-binary.md             # Definition of done (FR-002, FR-005–FR-007, FR-009, FR-010)
│   ├── non-goals.md                # Product non-goals vs later-not-first (FR-004, FR-008)
│   ├── release-profile.md          # Cargo features, handler/type sets
│   ├── dialect-first-binary.md     # PG/Redis subsets vs 015 ceiling
│   ├── conformance-profile.md      # Test gates SC-001–SC-006
│   ├── milestone-record.md         # Changelog / deferred-slice contract (SC-004)
│   └── fixtures/
│       ├── first-binary-one-node.conf
│       ├── first-binary-three-node-a.conf
│       ├── first-binary-three-node-b.conf
│       ├── first-binary-three-node-c.conf
│       └── invalid/
│           ├── unknown-handler-cassandra.conf
│           ├── omitted-transport.conf
│           └── copy-begin-not-in-config.md   # runtime dialect, not config
├── checklists/requirements.md
└── tasks.md                        # Phase 2 (/speckit-tasks — NOT created here)
```

### Source Code (repository root)

```text
Cargo.toml                          # default features = first-binary handler set
crates/
├── release-profile/                # spacestorage-release-profile
│   └── src/
│       ├── lib.rs
│       ├── slice.rs                # SliceId 1..=11, FIRST_BINARY, DEFERRED_AFTER_FIRST
│       ├── profile.rs              # ReleaseProfile { FirstBinary, CompleteProduct }
│       ├── handlers.rs             # required vs forbidden handler names per profile
│       ├── types.rs                # required L3 creatable set for first binary
│       ├── nongoals.rs             # static non-goal ids for SC-005 audit
│       └── ledger.rs               # parse milestone YAML; require deferred list
├── config/                         # already from 001: unknown_handler; omitted transport
├── node/                           # registers only feature-enabled handlers
├── handler-postgresql/             # feature "handler-postgresql" (default on)
├── handler-redis/                  # feature "handler-redis" (default on)
├── handler-cassandra/              # optional; complete-product / slice 6
├── handler-elasticsearch/          # optional
├── handler-clickhouse/             # optional (native + HTTP)
├── handler-s3/                     # optional
├── handler-webdav/                 # optional
└── conformance/
    └── tests/
        ├── first_binary.rs         # 1-node, 3-node, PG/Redis, TWO, restore
        ├── dialect_pg.rs           # COPY/BEGIN/COMMIT/ROLLBACK → 0A000
        ├── dialect_redis.rs        # MUST list on KV; HGET/JSON.GET error
        ├── document_store.rs       # admin-create + canonical blob CRUD
        ├── handlers_absent.rs      # cassandra entrypoint → unknown_handler
        └── multi_active.rs         # create-on refused

docs/
├── milestones/
│   ├── README.md                   # how to record a slice milestone
│   └── 000-template.md             # deferred-slice section required
└── examples/
    ├── first-binary-one-node.conf
    └── first-binary-three-node/    # copies of the fixtures
```

**Structure Decision**: Keep a single workspace and a single server binary. The first binary is a **default feature set** plus a **conformance profile**, not a fork. `release-profile` is the only new crate: it encodes slice IDs, handler/type sets, and ledger validation so `config`, `node`, and `conformance` do not hard-code magic strings. Protocol, type, WAL, membership, and quorum **behavior** stay in `001`–`015`; this feature only **selects and proves** the 1–5 subset.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| XII: no Raft in the first binary | Slices 1–5 need three-node quorum and membership before the control-plane crate (`06`) exists | Implementing Raft inside this feature would duplicate `06` and delay the first binary; constitution already sequences controller elections as their own feature. Interim `ClusterStore` from `004`/`011` is the seam `06` replaces. Recorded as deferred slice 7. |
| XIII: incomplete role/quota vocabulary in the first binary | SCRAM, Redis AUTH, admin token, master-key file, and explicit TLS/plaintext are enough to run PG+Redis smokes | Full `14` vocabulary + `07` quotas would pull Raft-backed role storage into slice 1–5. Deferred as slice 7; interim auth seams from `001`/`002`/`014` remain. |
| V: five client handlers absent from the default binary | First binary is not the seven-protocol matrix (clarification Q1; constitution V points here) | Shipping stub handlers that return empty success would violate `15` (silent success forbidden) and confuse operators. Absent handler + `entrypoint_unknown_handler` is the specified failure. Slice 6 still owes the `15` MUST subset. |
| X: full `08` catalog not live | Only implemented paths have series; slice 9 owns the catalog | Emitting placeholder series for unimplemented families would create false operator contracts. Additions only; no renames. |

## Phase 0 — Research

See [research.md](research.md). All Technical Context items were resolved from the spec (including Session 2026-09-18), constitution V/VIII/XII notes, sibling specs `001`–`015`, and the `004` multi-node harness pattern. No `NEEDS CLARIFICATION` remains.

## Phase 1 — Design

- [data-model.md](data-model.md): Slice, ReleaseProfile, MilestoneRecord, ProductNonGoal, HandlerBuildSet, DialectProfile, StarterTopology.
- [contracts/](contracts/): slice ledger, first-binary DoD, non-goals, Cargo features, dialects, conformance, milestone records, starter fixtures.
- [quickstart.md](quickstart.md): operator path mapped to SC-001 and Story 1.

## Constitution Check (post-design)

Re-evaluated after Phase 1 (2026-09-18 clarifications): the design is a profile and gate over the existing monolith, not a second product. Default binary omits five handlers by feature flag (constitution V first-binary clause). Product write TWO is unchanged; the one-node starter is an explicit config override (constitution VIII). Document Store uses the existing canonical-blob mapping (`002`), not a second document engine. Raft, full authz, quotas, remaining handlers, query-beyond-CRUD, full metrics catalog, migration/PITR, and UIs/ingest are **deferred slices 6–11**, listed in the milestone contract so they cannot vanish. Product non-goals are explicit refuse/out-of-scope, not later. **Gate result: PASS.**

## Next Step

Re-run `/speckit-tasks` so `tasks.md` picks up Session 2026-09-18 (write-ONE starter, Document Store blob, PG txn trio, Redis extra-verb errors). An older `tasks.md` exists and is stale relative to this plan.
