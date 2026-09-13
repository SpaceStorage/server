# Implementation Plan: Multiparadigm Type System (L0–L4)

**Branch**: `003-type-system` | **Date**: 2026-09-13 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/003-type-system/spec.md` (with Clarifications, Session 2026-09-13)

## Summary

Turn `crates/types` — the seam `002` stubbed with six in-memory types — into the real SpaceStorage type system: **55 registered types across five levels**, each with a complete `TypeDescriptor` (level, kind, storage modes, operation set, capability compatibility, allowed layouts, canonical representation version, starter example) published through one queryable **catalog**, and each container created from a **definition** that the type system validates before a byte is allocated (type, schema, layout of L0 primitives, storage mode, per-field encodings, compression codec, encryption algorithm/key/scope, L1 capability declarations, L4 members).

Four new engine crates sit under the type library: `codec` (Gorilla, delta, dictionary, RLE + Snappy, ZSTD, LZ4), `crypto` (AES-256-GCM, ChaCha20-Poly1305 with a `KeyAuthority` seam for `07` and the clarified `drives` / `drives and memory` scope), `storage` (the block pipeline `encode → compress → encrypt`, the L0 storage primitives `memtable`/`wal`/`sstable`/`append_segment`, the `lsm_tree` layout, memory/persistent/hybrid modes, and boot-time restore), and `placement` (the L1 capability declaration model and compatibility matrix, validated here and executed by `04` behind a `PlacementDirector` seam). Four type libraries — `l0` (15 data structures, all directly creatable per Clarification Q1), `l2` (18 abstractions), `l3` (10 storage models), `l4` (7 composition kinds with per-kind write rules per Clarification Q3) — register into the catalog through an explicit builder in `typeset`.

The type system also becomes the owner of **container schema** (Clarification Q4): columns, fields, field paths, key definitions, vector dimensions and value domains live with the container definition, evolve additively in place, and are the only schema a driver may render. Everything a driver or the planner needs stays behind the `002` abstract datatype interface, extended additively (new methods with defaults, never a rename), so the eight protocol handlers compile unchanged and gain 49 new types for free. Memory mode is implemented as an explicitly volatile tier (Clarification Q2): definitions and options are restored at boot, content is not.

## Technical Context

**Language/Version**: Rust 1.87 (edition 2024, MSRV 1.85), same workspace as `001`/`002`.

**Primary Dependencies** (all pure Rust; `*-sys` and C-backed crates prohibited, enforced by `cargo-deny` in CI): `snap` (Snappy), `structured-zstd` (pure-Rust Zstandard, full level range, C-zstd-compatible frames; `ruzstd` decoder kept as a cross-check in tests), `lz4_flex` (already in `002`); `aes-gcm`, `chacha20poly1305`, `hkdf`, `sha2`, `zeroize`, `subtle`, `rand_core`/`getrandom` (RustCrypto); `roaring` (bitmap/bitset and bitmap index payloads), `kiddo` (KD-tree), `rstar` (R-tree for the spatial index primitive), `unicode-segmentation` + `unicode-normalization` (tokenisation for n-gram and full-text), `crc32fast` and `xxhash-rust` (block and segment checksums), `memmap2` (optional read path for SSTable blocks, pure Rust); `serde`/`serde_json`, `ryu`, `bytes`, `tokio`, `async-trait`, `tracing`, `uuid` (v7 container identity), `chrono`, `parking_lot`. Dev-dependencies only: `instant-distance` and `fast-hnsw` (recall oracles for the in-house HNSW), `vicinity` (ScaNN-style oracle), `proptest` (canonical and codec round-trip properties), `tempfile`.

**Storage**: This is the feature that introduces storage. Node-local data under `storage { data_dir … }`: `catalog/` (append-only metadata log + snapshots, the source of definitions and schemas at boot), `ns/<namespace>/<container-id>/` (WAL segments, SSTables, append-only segments, index files). Memory mode allocates from the `types.memory` buffer re-homed from `002`; hybrid splits per the type's documented policy.

**Testing**: `cargo test`. Unit tests per crate (codec round-trips and golden vectors, block pipeline, LSM compaction, schema evolution matrix, capability matrix, canonical vectors extended to every type). `crates/conformance` gains a **catalog-driven** suite that enumerates the catalog rather than a hard-coded list: every type × every declared storage mode × every applicable encoding × every codec × both encryption scopes (SC-004), every catalog claim exercised through the abstract interface (SC-007, FR-049), every starter example run verbatim (SC-003), restart restore including the memory-mode emptiness assertion (SC-004), composition write rules (SC-009), additive and refused schema changes (SC-009a), registration isolation (SC-010), and a plaintext-scan of memory-resident regions for scope `drives and memory` (SC-011). The `002` cross-protocol matrix is re-run against the full inventory (its SC-002/SC-003 grow from 6 types to 50 container-creatable ones).

**Target Platform**: Linux x86_64/aarch64 servers (primary), macOS for development. Direct I/O is not used; page cache plus an explicit block cache buffer.

**Project Type**: Cargo workspace extension — nine new library crates, no new binaries; `config`, `node`, `admin-proto`, `spacestorage` (CLI) and `conformance` gain type-system surfaces.

**Performance Goals**: catalog read and descriptor lookup < 1 ms (in-memory, no data touched — FR-039); container create/describe/drop < 50 ms p95 on a warm node; point read/write on `kv_store` ≥ 50 k ops/s per node on loopback with default options; boot restore of 10 000 container definitions < 30 s (FR-023); additive schema change applies without rewriting data and completes < 100 ms regardless of container size (FR-017b); encryption scope `drives and memory` costs < 25 % throughput against the same container at scope `drives` (documented, measured, not a gate).

**Constraints**: no blocking I/O on Tokio worker threads — WAL append, SSTable read and write go through `tokio::fs`, while compaction, index build and ANN graph construction run on `spawn_blocking` with a bounded pool; a container's type is immutable and enforced structurally (no `set_type`); validation is whole-definition and allocation-free on failure (FR-018); compression always precedes encryption (FR-026) and the block header carries codec, key id and nonce; key material never leaves `crates/crypto` and never enters a descriptor, log, admin response or config output (SC-011, enforced by a `Zeroizing` newtype without `Debug`/`Display`); every type descriptor must be exercised by the conformance harness or CI fails (FR-049); the abstract datatype interface may only grow (default-bodied methods), never rename, so `002` handlers keep compiling.

**Scale/Scope**: 55 types (15 L0 data structures + 4 storage primitives + 1 layout + 18 L2 + 10 L3 + 7 L4), of which 50 are directly creatable; 4 encodings, 3 codecs + none, 2 encryption algorithms, 2 scopes, 13 shared capabilities; 9 new crates; largest items are the LSM/storage engine (~6 k lines), the L3 models (~8 k), the L2 abstractions (~6 k), HNSW + ScaNN (~4 k), full-text/n-gram indexing (~3 k), codecs (~2.5 k), catalog and restore (~2.5 k), schema and validation (~2 k); ≈ 60–80 k lines including tests.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Gate | Status |
|---|-----------|------|--------|
| I | Rust-Only Stack | Every dependency above is pure Rust; ZSTD via `structured-zstd`/`ruzstd` rather than `zstd-sys`; `croaring` rejected for `roaring`; `simsimd` feature of ANN crates never enabled; CI runs `cargo deny` with a ban on `*-sys` and a `cargo tree` assertion | PASS |
| II | Fully Asynchronous Tokio Runtime | All container operations are `async` on the `001` runtime; file I/O via `tokio::fs`; CPU-heavy work (compaction, ANN build, dictionary training, re-encryption) on a bounded `spawn_blocking` pool so worker threads never stall | PASS |
| III | Single-Process Multithreaded Monolith | Nine library crates linked into `spacestoraged`; no sidecars, no new binaries; compaction runs as Tokio tasks, not processes | PASS |
| IV | Type-Driven Multiparadigm | This feature **is** the principle: five levels, one descriptor per type, expandable registries for types, encodings, codecs, algorithms and capabilities; L1 stays a capability layer over types (`crates/placement`), never a second database | PASS |
| V | Protocol Compatibility on Distinct Ports | Unchanged; the type system is protocol-agnostic and reaches drivers only through the `002` abstract interface, which grows additively so no handler changes | PASS (N/A) |
| VI | Every Node Is a Request Coordinator | Type operations are local; remote access is `04`/`05`'s routing. Container identity (`uuid` v7) and the catalog are node-local now, cluster-wide under `06`, behind a `CatalogStore` seam | PASS |
| VII | Label-Based Planetary Placement | Capability declarations (replication factor, anti-affinity label, sharding key, persistent placement, labels) are parsed, validated against the per-type matrix, stored with the definition and handed to `PlacementDirector`; disks and memory enter as `memory { size; labels; }` and `storage { data_dir; }` with label semantics owned by `04` | PASS |
| VIII | Cassandra-Style Quorum with Protocol Defaults | Quorum stays on the query (`002`/`05`); this feature only declares which types accept the `quorum` and `consistency` capabilities and forwards `PlacementInfo` | PASS |
| IX | Multi-Tenant Namespaces | Every container belongs to exactly one namespace; names unique per namespace; compositions namespace-scoped; per-type counters carry `namespace` and `container` labels for `07` quotas and `08` series | PASS |
| X | Observability as a Product Surface | FR-044 counters (access, hits/misses, duration, resource usage) tracked per type and namespace in `node::stats` with `08`-conformant reserved names; buffers for memtable, block cache, WAL and catalog are registered and monitored | PASS |
| XI | Documented, Expandable Configuration | `contracts/config-directives.md`, a starter `node-typed.conf`, a starter example per type in the descriptor (SC-003), and registration paths for types, encodings, codecs, algorithms and capabilities (FR-045–FR-049) | PASS |
| XII | Raft Controller Elections and Local Restore | FR-023 restore is implemented here: catalog log + snapshot replay, WAL replay per persistent container, memory-mode containers restored empty and described as such; controller elections remain `06` | PASS |
| XIII | Security Defaults for Data and Roles | Per-container encryption with algorithm, key reference and scope; key material resolved through a `KeyAuthority` seam with an interim keyring file until `07`; unresolvable key ⇒ container present but unavailable, never plaintext. Interim keyring tracked below | PASS with justified deviation |
| Arch. Contracts | Five-level stack respected, inventories expandable, no parallel hierarchy | `Level` is a closed enum of L0–L4; registration requires exactly one level (FR-045); L4 distribution kinds declare the L1 capabilities they compose and add none (FR-008) | PASS |
| Observability Contract | No `08` label renamed or dropped | Only additions (`type`, `level`, `container`, `mode`, `codec`) alongside `08`'s own `namespace`/`node` | PASS |

**Gate result (pre-research)**: PASS. Deviations justified in Complexity Tracking: interim key authority (XIII), interim single-node placement director (sequencing against `04`), node-local catalog store (sequencing against `06`).

## Project Structure

### Documentation (this feature)

```text
specs/003-type-system/
├── plan.md                                  # This file
├── research.md                              # Phase 0: R1–R22
├── data-model.md                            # Phase 1: descriptors, catalog, container, schema, layout, codecs, capabilities, compositions, state machines
├── quickstart.md                            # Phase 1: create → configure → compose → restart walkthrough per success criterion
├── contracts/
│   ├── type-catalog.md                      # TypeDescriptor schema, catalog API, registration and deprecation (FR-009–FR-013, FR-045–FR-049)
│   ├── type-inventory.md                    # All 55 types: name, level, kind, creatable, default layout, modes, ops, capabilities
│   ├── abstract-datatype-interface.md       # v2 of 002's contract: additive trait growth, capability descriptors, schema access
│   ├── container-definition.md              # Flat option namespace, DDL mapping, validation codes (FR-015–FR-020)
│   ├── schema.md                            # Value domains, schema kinds, additive evolution matrix (FR-017a, FR-017b)
│   ├── storage-layout.md                    # Storage modes, block pipeline, LSM/WAL/SSTable formats, catalog store, restore (FR-021–FR-023, FR-029)
│   ├── codecs.md                            # Encoding and compression registries, applicability by data kind (FR-024–FR-026)
│   ├── encryption.md                        # Algorithms, scope semantics, KeyAuthority seam, key lifecycle (FR-027, FR-027a, FR-028)
│   ├── shared-capabilities.md               # L1 declaration model, compatibility matrix, PlacementDirector seam (FR-030–FR-036)
│   ├── composition.md                       # L4 kinds, membership, write rules, refresh, nesting (FR-007, FR-008, FR-008a)
│   ├── canonical-payloads.md                # Canonical representation v1 extended to every type (FR-042)
│   ├── config-directives.md                 # storage/memory/types/keys blocks, buffers, validation codes, admin + CLI additions
│   └── fixtures/
│       ├── node-typed.conf                  # Starter node with storage, memory, types, keys
│       ├── keyring.example
│       ├── containers/*.def                 # One definition example per level (used by the SC-003 harness)
│       ├── README.md
│       └── invalid/*                        # One file per new validation code
├── checklists/requirements.md
└── tasks.md                                 # Phase 2 (/speckit-tasks — NOT created here)
```

### Source Code (repository root)

```text
Cargo.toml                                   # workspace: adds the crates below
crates/
├── config/                                  # (001/002) + storage{}, memory{}, types{}, keys{} blocks; new validation codes
├── node/                                    # (001/002) + TypeSystem construction, restore orchestration, per-type stats, new buffers
├── admin-proto/                             # (001/002) + Catalog, TypeDescriptor, ContainerDescription DTOs; ops types/type/containers/describe/catalog
├── spacestorage/                            # (001 CLI) + `types`, `type`, `containers`, `describe`, `catalog` commands
│
├── types/                                   # spacestorage-types — CORE (002's seam, now the real owner)
│   └── src/
│       ├── lib.rs                           # TypeSystem, TypeRegistry (002-compatible), builder + registration (FR-045–FR-048)
│       ├── descriptor.rs                    # TypeDescriptor, Level, Kind, StorageModes, OperationSpec, CapabilitySupport, Deprecation
│       ├── datatype.rs                      # Datatype / Container / ObjectOps traits — 002 signatures + additive methods
│       ├── definition.rs                    # ContainerDefinition, Layout, EncodingChoice, CompressionChoice, EncryptionChoice
│       ├── schema.rs                        # ContainerSchema, Field, ValueDomain, KeyDef, evolution rules
│       ├── validate.rs                      # whole-definition validation pipeline → TypeError codes (FR-018)
│       ├── catalog.rs                       # TypeCatalog (descriptors) + ContainerCatalog (definitions, identity, dependants)
│       ├── canonical.rs                     # canonical v1 from 002 + per-type payload schema registry (FR-042)
│       ├── ident.rs                         # NamespaceName, ContainerName, ContainerRef, ContainerId (uuid v7)
│       ├── ops.rs                           # capability descriptors, operation dispatch, NotSupportedByType (FR-039–FR-041)
│       ├── stats.rs                         # per-type access/hit/miss/duration/resource counters (FR-044)
│       └── error.rs                         # TypeError (002 variants preserved + new codes)
│
├── codec/                                   # spacestorage-codec
│   └── src/
│       ├── lib.rs                           # EncodingRegistry, CompressionRegistry, DataKind applicability (FR-024, FR-025)
│       ├── encoding/{plain,gorilla,delta,dictionary,rle}.rs
│       └── compression/{none,snappy,zstd,lz4}.rs
│
├── crypto/                                  # spacestorage-crypto
│   └── src/
│       ├── lib.rs                           # AlgorithmRegistry, EncryptionScope, SealedBlock header
│       ├── aead/{aes_gcm,chacha}.rs         # per-block nonce, key id, AAD = block coordinates
│       ├── key_authority.rs                 # KeyAuthority trait (07 seam) + KeyRef resolution/caching
│       └── keyring_file.rs                  # interim provider (0600 file), live re-read
│
├── storage/                                 # spacestorage-storage — L0 engines and durability
│   └── src/
│       ├── mode.rs                          # memory | persistent | hybrid; hybrid policies
│       ├── block.rs                         # encode → compress → encrypt pipeline, block header, checksums
│       ├── memtable.rs  wal.rs  sstable.rs  segment.rs
│       ├── lsm/{mod,compaction,manifest}.rs
│       ├── cache.rs                         # block cache against buffer storage.block_cache
│       ├── catalog_store.rs                 # append-only catalog log + snapshot (06 seam)
│       └── restore.rs                       # boot restore: definitions always, content per mode (FR-023)
│
├── placement/                               # spacestorage-placement — L1 seam (04 owner)
│   └── src/
│       ├── lib.rs                           # SharedCapability, CapabilityDecl, CapabilityParams
│       ├── matrix.rs                         # per-type compatibility, fixed-after-create parameters (FR-031, FR-035)
│       ├── director.rs                      # PlacementDirector trait + PlacementInfo bridge to 002's exec seam
│       └── local.rs                         # interim single-node director (replicas = 1)
│
├── l0/                                      # 15 data structures + 4 storage primitives + lsm_tree descriptors
│   └── src/{tuple,vector,linked_list,deque,ring_buffer,hash_table,bplus_tree,skip_list,radix_tree,heap,bloom_filter,kd_tree,hnsw,scann,bitmap}.rs
│       └── {memtable,sstable,wal,append_segment,lsm_tree}.rs   # non-creatable descriptors + layout roles
├── l2/                                      # 18 abstractions
│   └── src/{map,ordered_map,multimap,set,ordered_set,sequence,document,field_index,field_path,kv_collection,bitmap_index,ngram_index,range_index,spatial_index,vector_collection,timeseries_segment,object,object_collection}.rs
├── l3/                                      # 10 storage models
│   └── src/{relational_table,columnar_table,document_store,fulltext_search,vector_search,spatial_search,kv_store,timeseries,object_storage,log_stream}.rs
├── l4/                                      # 7 composition kinds
│   └── src/{union,federated,materialized_view,distributed,partitioned,replicated,sharded}.rs
├── typeset/                                 # assembles l0+l2+l3+l4 into a TypeSystem (single registration point, no link-time magic)
│
└── conformance/                             # (002) + catalog-driven type suites
    └── tests/
        ├── catalog_complete.rs              # SC-001 (every inventory name, every descriptor field)
        ├── starter_examples.rs              # SC-003
        ├── matrix_modes_codecs.rs           # SC-004 (type × mode × encoding × codec × 2 key/algorithm pairs)
        ├── invalid_definitions.rs           # SC-005 (one case per validation code, nothing allocated)
        ├── capability_matrix.rs             # SC-006
        ├── operations_reachable.rs          # SC-007, FR-049 (every catalog claim invoked, metadata-only capability queries)
        ├── canonical_all_types.rs           # SC-008
        ├── compositions.rs                  # SC-009 (kinds, cycles, cross-namespace, write rules)
        ├── schema_evolution.rs              # SC-009a
        ├── registration_isolation.rs        # SC-010
        ├── encryption_scope.rs              # SC-011 (no key material anywhere; memory plaintext scan)
        ├── restart_restore.rs               # FR-023 (persistent/hybrid intact; memory empty and described)
        └── type_stats.rs                    # SC-012

docs/
├── types/                                   # generated from contracts at release: one page per type with its starter example
├── storage.md                               # modes, codecs, encryption scopes, restore
└── examples/node-typed.conf
```

**Structure Decision**: `crates/types` keeps the crate name and trait signatures `002` already depends on, so the eight handlers compile against the real system without edits; everything new arrives as default-bodied trait methods and extra registry entries. The engine crates (`codec`, `crypto`, `storage`) sit **below** the type libraries and know nothing about levels, which keeps the block pipeline reusable by every primitive and prevents a per-type storage fork. The type libraries are split by level (`l0`, `l2`, `l3`, `l4`) so the dependency graph enforces the architecture: `l2` may depend on `l0`, `l3` on `l2`/`l0`, `l4` on none of them (it composes through `ContainerRef` only), and nothing depends upward. `typeset` is the single place where registration happens, making FR-046 (registration changes nothing existing) a compile-time-visible property. `placement` is a thin seam crate so `04` can replace `local.rs` without touching a type.

## Complexity Tracking

| Violation / deviation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| Interim `keyring_file` key authority (XIII) | FR-027 requires per-container algorithm/key/scope now; `07` owns key management and does not exist | Skipping encryption leaves FR-027/FR-027a unimplemented and the constitution's XIII unmet. The file is `0600`, referenced by path, never inlined, and `KeyAuthority` is the single trait `07` replaces; keys are held in `Zeroizing` buffers with no `Debug`. |
| Interim single-node `PlacementDirector` (sequencing vs `04`) | FR-030–FR-035 require declarations to be validated and stored now; capability *semantics* are `04`'s | Deferring the declaration model would push placement parsing into drivers or `04`-specific types, recreating the parallel hierarchy the constitution forbids. The matrix and declaration storage are type-system-owned; only execution is stubbed (replicas = 1). |
| Node-local `CatalogStore` (sequencing vs `06`) | FR-023 boot restore and FR-011 per-node catalog need durable metadata now; cluster catalog leadership is `06` | A cluster catalog without controllers would invent a second consensus mechanism. The append-only log + snapshot is local, and `CatalogStore` is the seam `06` fronts with a controller. |
| In-house HNSW and ScaNN rather than depending on an ANN crate | L0 primitives must support memory/persistent/hybrid modes and our encoding/compression/encryption pipeline; the ANN crates own their own serialization and file I/O | Wrapping `hnsw_rs`/`vicinity` would put two storage stacks in one product and make FR-021/FR-024 unimplementable for those types. The crates are kept as **dev-dependency recall oracles** so correctness is measured, not asserted. ScaNN is the largest single item and is sequenced last. |
| `structured-zstd` (young crate) for a required codec | FR-024 makes ZSTD mandatory and Principle I forbids `zstd-sys` | Dropping ZSTD violates FR-024; using the C binding violates Principle I. Mitigation: frames are validated against the independent `ruzstd` decoder in property tests, the codec registry can pin a level range, and the codec is swappable behind `CompressionRegistry`. |
| 55 types in one feature | The inventory **is** the architecture (Principle IV); a partial inventory makes FR-001, FR-013, FR-018 and the `002` driver-coverage check unverifiable | Splitting by level was considered and rejected: `002` already fails startup when a driver lacks a mapping for a registered type, so a half-registered inventory would either break `002` or require a temporary exemption. Instead the work is sequenced in waves below, all inside this feature, with the catalog and conformance harness landing first so every later wave is verified on arrival. |

## Phase 0 — Research

See [research.md](research.md), R1–R22. All Technical Context items are resolved; no `NEEDS CLARIFICATION` remains. Key outcomes: crate decomposition and the additive-growth rule for the `002` interface (R1, R2); type naming and the 55-name vocabulary (R3); descriptor schema and registration mechanism (R4, R5); container identity and the catalog store format (R6, R7); storage modes and hybrid policies (R8); block pipeline ordering and header layout (R9); codec selection and applicability (R10, R11); encryption algorithms, scope implementation and key seam (R12, R13); schema model, value domains and the widening matrix (R14, R15); capability matrix and placement seam (R16); composition semantics, identity and refresh (R17); capability descriptors and metadata-only answers (R18); canonical payload extension (R19); ANN, full-text and spatial implementation strategy with oracles (R20); per-type metrics naming (R21); conformance harness generation from the catalog (R22).

## Phase 1 — Design

- [data-model.md](data-model.md): the 15 entities of the spec as concrete records with fields, invariants and relationships, plus four state machines (container lifecycle, schema evolution, restore, materialized-view refresh).
- [contracts/](contracts/): catalog and descriptor schema, the full 55-type inventory table, the v2 abstract interface, the flat container-definition option namespace with validation codes, the schema and evolution matrix, storage layout and restore formats, codec and encryption contracts, the capability matrix and placement seam, composition rules, canonical payloads for every type, configuration additions, and fixtures.
- [quickstart.md](quickstart.md): a walkthrough that creates one container per level, configures mode/encoding/compression/encryption, declares capabilities, composes an L4 object, evolves a schema, restarts the node, and reads the catalog — mapped to every success criterion.

## Constitution Check (post-design)

Re-evaluated after Phase 1: no new violations. The design keeps every dependency pure Rust with a CI ban on `*-sys`; all container operations async with CPU-heavy work on a bounded blocking pool; one process; five levels with a closed `Level` enum and open registries; L1 as declarations plus a matrix rather than a second database; namespace-scoped containers and per-type/per-namespace counters; local restore of every definition with honest reporting of memory-mode content; per-container encryption with a seam for `07`; and documented, expandable configuration with a starter example per type. The three interim seams and the two dependency deviations stand as tracked above. **Gate result: PASS.**

## Risks and sequencing notes

- **Inventory size is the schedule.** Suggested waves for `/speckit-tasks`: (1) `types` core — descriptors, catalog, validation, canonical, stats — plus the conformance harness that reads the catalog, so every later wave arrives verified; (2) `codec`, `crypto`, `storage` block pipeline and the memory mode; (3) L0 data structures except HNSW/ScaNN, with `lsm_tree`, WAL, SSTable and restore; (4) L2 abstractions; (5) L3 models (relational, columnar, document, K/V, time series, object, log stream); (6) index models (full-text, spatial) and the ANN primitives with their oracles, then vector search; (7) `placement` matrix and L4 compositions; (8) config, admin, CLI, docs and the `002` re-run at full inventory.
- **`002` interface growth**: every new trait method must have a default body, or the eight handlers stop compiling. A CI job builds `002`'s handler crates against each commit of `crates/types` to catch a signature change early.
- **`002` interim types retire here**: `kv_collection`, `relational_table`, `document_store`, `object_collection`, `vector_collection` and `ordered_map` keep their machine names and canonical `$type` strings, so `002`'s canonical vectors and per-protocol mapping tables stay valid; their in-memory implementations are deleted, which is the moment data first survives a restart.
- **`Field Path` as a container** is the least obvious inventory entry; research R3 fixes it as a path→value store over a radix tree, usable standalone and as an index component, so that "every L2 abstraction is creatable" holds without inventing a pseudo-type.
- **ScaNN** is experimental everywhere in the Rust ecosystem; if wave 6 slips, the type ships with its descriptor marked `deprecated: false, availability: preview` and the conformance harness still exercises every claim it makes — a descriptor that overstates capability is a CI failure (FR-049), so a preview type must under-claim rather than over-claim.
- **Encryption scope `drives and memory`** has a real cost; the quickstart measures it so operators choose with numbers, and the exempt "transient per-operation buffer" boundary is written down in `contracts/encryption.md` rather than left to implementation.
- **Cluster catalog divergence** (FR-011 per-node differences) can only be partially tested before `06`; the per-node reporting path is built and the cluster aggregation test is written and `#[ignore]`d with its reason, as `002` did for SC-007.

## Next Step

Run `/speckit-tasks` to generate `tasks.md` from this plan and the Phase 1 artifacts.
