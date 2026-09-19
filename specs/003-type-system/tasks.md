---
description: "Task list for Multiparadigm Type System (L0–L4) implementation"
---

# Tasks: Multiparadigm Type System (L0–L4)

**Input**: Design documents from `/specs/003-type-system/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md), [data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: Requested by the feature. Spec Independent Tests + SC-001–SC-012 + FR-049 require a catalog-driven conformance harness under `crates/conformance/tests/`. Plan lists the suite files. Write failing tests first within each story phase that includes them. Unit tests inside engine crates (codec round-trips, block pipeline, schema evolution) are part of those crates' implementation tasks where noted.

**Scope of this feature**: Turn `crates/types` into the real type system (55 types, five levels), add engine crates `codec` / `crypto` / `storage` / `placement`, type libraries `l0` / `l2` / `l3` / `l4`, assembler `typeset`, and wire config / node / admin / CLI / docs. Do not implement `04` placement semantics, `06` cluster catalog, `07`/`14` key authority beyond the interim keyring, or `10` transforms.

**Sibling continuity**: Keep `002` machine names and additive trait growth (`kv_collection`, `relational_table`, `document_store`, `object_collection`, `vector_collection`, `ordered_map`). Delete in-memory-only stubs when real implementations land.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: [US1]…[US7] on user-story phase tasks only
- Every task includes an exact file path

## Path Conventions

Workspace root paths from [plan.md](plan.md): `crates/{types,codec,crypto,storage,placement,l0,l2,l3,l4,typeset,config,node,admin-proto,spacestorage,conformance}/`, `docs/types/`, `docs/storage.md`, `docs/examples/`.

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Workspace members, crate skeletons, and dependency inventory from the plan (pure Rust only; `cargo-deny` ban on `*-sys`).

- [ ] T001 Create `crates/codec/Cargo.toml` (package `spacestorage-codec`, edition 2024) and `crates/codec/src/lib.rs` with module stubs `encoding`, `compression`
- [ ] T002 [P] Create `crates/crypto/Cargo.toml` (package `spacestorage-crypto`) and `crates/crypto/src/lib.rs` with module stubs `aead`, `key_authority`, `keyring_file`
- [ ] T003 [P] Create `crates/storage/Cargo.toml` (package `spacestorage-storage`) and `crates/storage/src/lib.rs` with module stubs `mode`, `block`, `memtable`, `wal`, `sstable`, `segment`, `lsm`, `cache`, `catalog_store`, `restore`
- [ ] T004 [P] Create `crates/placement/Cargo.toml` (package `spacestorage-placement`) and `crates/placement/src/lib.rs` with module stubs `matrix`, `director`, `local`
- [ ] T005 [P] Create `crates/l0/Cargo.toml` (package `spacestorage-l0`), `crates/l2/Cargo.toml`, `crates/l3/Cargo.toml`, `crates/l4/Cargo.toml`, and `crates/typeset/Cargo.toml` with empty `src/lib.rs` each
- [ ] T006 Add all nine new crates to workspace `[workspace.members]` in `Cargo.toml` and declare shared deps from plan Technical Context (`snap`, `structured-zstd`, `lz4_flex`, `aes-gcm`, `chacha20poly1305`, `hkdf`, `sha2`, `zeroize`, `subtle`, `roaring`, `kiddo`, `rstar`, `unicode-segmentation`, `unicode-normalization`, `crc32fast`, `xxhash-rust`, `memmap2`, `serde`, `bytes`, `tokio`, `async-trait`, `tracing`, `uuid`, `chrono`, `parking_lot`) plus `cargo-deny` ban on `*-sys`
- [ ] T007 [P] Copy [contracts/fixtures/node-typed.conf](contracts/fixtures/node-typed.conf) to `docs/examples/node-typed.conf` and [contracts/fixtures/keyring.example](contracts/fixtures/keyring.example) to `docs/examples/keyring.example`
- [ ] T008 [P] Create `docs/types/README.md` (one page per type generated from catalog at release) and stub `docs/storage.md` for modes/codecs/encryption/restore

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core type-system, engine seams, and registration builder that MUST exist before any user story. No story work until this phase completes.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [ ] T009 Implement closed `Level` (`L0`…`L4`) and `Kind` enums in `crates/types/src/descriptor.rs` with derived `creatable` (`DataStructure | Abstraction | StorageModel | Composition` only; `StoragePrimitive`/`StorageLayout`/`SharedCapability` never creatable) per [data-model.md](data-model.md) §1 and FR-001/FR-014
- [ ] T010 Implement `TypeDescriptor` and `OperationSpec` in `crates/types/src/descriptor.rs` with all mandatory fields from [type-catalog.md](contracts/type-catalog.md) (`name`, `display_name`, `level`, `kind`, `storage_modes`, `schema_kind`, `schema_required_elements`, `operations`, `capabilities`, `composed_from`, `default_layout`, `encodings`, `codecs`, `default_codec`, `encryption`, `canonical`, `deprecation`, `availability`, `starter_example`)
- [ ] T011 [P] Implement identity types `NamespaceName`, `ContainerName`, `ContainerRef`, `ContainerId` (UUID v7) in `crates/types/src/ident.rs`
- [ ] T012 [P] Implement `TypeError` in `crates/types/src/error.rs` preserving all `002` variants and adding codes from [container-definition.md](contracts/container-definition.md) (`UnknownType`, `NotCreatable`, `TypeDeprecated`, `AlreadyExists`, `SchemaRequired`, `UnsupportedStorageMode`, `LayoutComponentNotAllowed`, `EncodingNotApplicable`, `UnknownCodec`, `UnknownAlgorithm`, `EncryptionScopeInvalid`, `KeyUnresolvable`, `CapabilityUnsupported`, `CapabilityConflict`, `CompositionCycle`, `CompositionCrossNamespace`, `CompositionDepthExceeded`, `TypeImmutable`, `IncompatibleSchemaChange`, `IncompleteDescriptor`, `MultipleLevels`)
- [ ] T013 Implement `ContainerDefinition`, `Layout`, `EncodingChoice`, `CompressionChoice`, `EncryptionChoice` flat option namespace in `crates/types/src/definition.rs` matching [container-definition.md](contracts/container-definition.md) (`type` mandatory; `mode` ∈ `memory|persistent|hybrid`; `encryption.scope` ∈ `drives|drives_and_memory` default `drives`; `compression` ∈ `none|snappy|zstd|lz4`)
- [ ] T014 Implement `ContainerSchema`, `Field`, `ValueDomain`, `KeyDef` in `crates/types/src/schema.rs` with domains at least `bool`, `int32`, `int64`, `uint64`, `float32`, `float64`, `decimal`, `utf8`, `bytes`, `uuid`, `timestamp` (UTC µs), `date`, `jsonb`, `null` and distinct null/missing/tombstone states per FR-024a; vector domains require `dimension` and `metric ∈ l2|cosine|inner_product`
- [ ] T015 Implement whole-definition validation pipeline stages 1–11 in `crates/types/src/validate.rs` (allocation-free on failure; no catalog/storage/placement until all stages pass) per [container-definition.md](contracts/container-definition.md) §2
- [ ] T016 Implement `TypeCatalog` / `ContainerCatalog` / `TypeSystem` builder in `crates/types/src/catalog.rs` and `crates/types/src/lib.rs` with registries for types, encodings, codecs, algorithms, capabilities and `release` id (FR-010, FR-011, FR-045–FR-048)
- [ ] T017 [P] Extend `Datatype` / `Container` / `ObjectOps` in `crates/types/src/datatype.rs` **additively only** (default-bodied methods; never rename) per [abstract-datatype-interface.md](contracts/abstract-datatype-interface.md) — CI must keep `002` handlers compiling
- [ ] T018 [P] Implement capability-descriptor ops dispatch and `NotSupportedByType` in `crates/types/src/ops.rs` (metadata-only answers, FR-039–FR-041)
- [ ] T019 [P] Implement canonical representation registry v1 extension hooks in `crates/types/src/canonical.rs` (FR-042)
- [ ] T020 [P] Implement per-type/per-namespace counters in `crates/types/src/stats.rs` (`access`, hit/miss where applicable, duration, resource usage — FR-044)
- [ ] T021 Implement `EncodingRegistry` / `CompressionRegistry` and `DataKind` applicability in `crates/codec/src/lib.rs` with encodings `plain`, `gorilla`, `delta`, `dictionary`, `rle` under `crates/codec/src/encoding/` and codecs `none`, `snappy`, `zstd` (`structured-zstd`), `lz4` under `crates/codec/src/compression/` (FR-024–FR-026; compression before encryption is enforced by the block pipeline, not reversed here)
- [ ] T022 Implement `AlgorithmRegistry`, `EncryptionScope`, `SealedBlock` header, AEAD modules `crates/crypto/src/aead/aes_gcm.rs` and `crates/crypto/src/aead/chacha.rs`, `KeyAuthority` trait in `crates/crypto/src/key_authority.rs`, and interim `0600` keyring in `crates/crypto/src/keyring_file.rs` with `Zeroizing` key material that has no `Debug`/`Display` (FR-027, FR-027a, SC-011)
- [ ] T023 Implement storage `Mode` (`memory|persistent|hybrid`) in `crates/storage/src/mode.rs` and block pipeline `encode → compress → encrypt` with header (codec, key id, nonce, checksum) in `crates/storage/src/block.rs` (FR-026 ordering fixed)
- [ ] T024 Implement L0 storage engines `crates/storage/src/memtable.rs`, `wal.rs`, `sstable.rs`, `segment.rs` and `crates/storage/src/lsm/{mod,compaction,manifest}.rs` plus block cache against buffer `storage.block_cache` in `crates/storage/src/cache.rs`
- [ ] T025 Implement append-only `CatalogStore` (log + snapshot) in `crates/storage/src/catalog_store.rs` and boot restore in `crates/storage/src/restore.rs` (definitions always; content per mode — FR-023)
- [ ] T026 Implement `SharedCapability`, `CapabilityDecl`, per-type compatibility matrix in `crates/placement/src/matrix.rs`, `PlacementDirector` trait in `crates/placement/src/director.rs`, and interim single-node director (`replicas = 1`) in `crates/placement/src/local.rs` (FR-030–FR-036)
- [ ] T027 Implement `typeset` builder in `crates/typeset/src/lib.rs` as the **only** registration entry point assembling `l0`+`l2`+`l3`+`l4` into a `TypeSystem` (FR-046 isolation visible at compile time)
- [ ] T028 Extend `crates/config` with `storage{}`, `memory{}`, `types{}`, `keys{}` blocks and validation codes from [config-directives.md](contracts/config-directives.md); wire TypeSystem construction and restore orchestration hooks in `crates/node`
- [ ] T029 Add conformance harness scaffolding that enumerates the catalog (not a hard-coded type list) under `crates/conformance/tests/` with empty modules `catalog_complete.rs`, `starter_examples.rs`, `matrix_modes_codecs.rs`, `invalid_definitions.rs`, `capability_matrix.rs`, `operations_reachable.rs`, `canonical_all_types.rs`, `compositions.rs`, `schema_evolution.rs`, `registration_isolation.rs`, `encryption_scope.rs`, `restart_restore.rs`, `type_stats.rs`

**Checkpoint**: Workspace builds; `TypeSystem::builder()` compiles; validation codes and engine traits exist; no creatable type implementations required yet. User stories may start.

---

## Phase 3: User Story 1 - Create a storage-model container that matches the workload (Priority: P1) 🎯 MVP

**Goal**: Ten L3 storage models are creatable with documented defaults, schema-owned where required, writable/readable through the abstract interface, describable/droppable, type-immutable, with unsupported ops refused before data access.

**Independent Test**: On a single node, create one container of each of the ten L3 models with no layout options, describe each, write/read a small dataset, invoke one operation from each model's operation set, drop each (spec US1 Independent Test / SC-002).

### Tests for User Story 1 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T030 [P] [US1] Add SC-002 L3 smoke tests in `crates/conformance/tests/starter_examples.rs` (or `l3_smoke.rs`): create/describe/write/read/one-op/drop for `relational_table`, `columnar_table`, `document_store`, `fulltext_search`, `vector_search`, `spatial_search`, `kv_store`, `timeseries`, `object_storage`, `log_stream` with no layout options
- [ ] T031 [P] [US1] Add invalid-definition cases in `crates/conformance/tests/invalid_definitions.rs` for `UnknownType`, `NotCreatable` (`sstable`/`wal`/`lsm_tree`), `AlreadyExists` same-namespace duplicate, `TypeImmutable` alter-type, `SchemaRequired` for schema-required types without schema, `NotSupportedByType` for an op outside the set — nothing allocated on failure (SC-005 / FR-018)
- [ ] T032 [P] [US1] Add additive vs refused schema evolution cases in `crates/conformance/tests/schema_evolution.rs` (add field / widen domain / add index succeed without rewrite; drop/rename/narrow/retype/key/dimension change → `IncompatibleSchemaChange` with transform hint — SC-009a)

### Implementation for User Story 1

- [ ] T033 [P] [US1] Implement non-creatable L0 storage-primitive and layout descriptors (`memtable`, `sstable`, `wal`, `append_segment`, `lsm_tree`) in `crates/l0/src/{memtable,sstable,wal,append_segment,lsm_tree}.rs` with `NotCreatable` listing consumers (FR-014)
- [ ] T034 [P] [US1] Implement creatable L0 data structures needed by L3 defaults in `crates/l0/src/{hash_table,bplus_tree,bloom_filter,radix_tree,kd_tree,bitmap,append_segment_ops}.rs` (and remaining `tuple`, `vector`, `linked_list`, `deque`, `ring_buffer`, `skip_list`, `heap` as directly creatable per Clarification Q1) with modes/ops from [type-inventory.md](contracts/type-inventory.md)
- [ ] T035 [US1] Implement L0 ANN primitives `hnsw` (stable) and `scann` (`availability: Preview`, under-claim until oracle gate) in `crates/l0/src/{hnsw,scann}.rs` with `metric ∈ l2|cosine|inner_product`; keep `instant-distance` / `fast-hnsw` / `vicinity` as **dev-dependency recall oracles only**
- [ ] T036 [P] [US1] Implement L2 abstractions used by L3 defaults in `crates/l2/src/{map,ordered_map,kv_collection,document,field_index,field_path,bitmap_index,ngram_index,range_index,spatial_index,vector_collection,timeseries_segment,object,object_collection,multimap,set,ordered_set,sequence}.rs` preserving `002` machine names and ops for the six continuity types
- [ ] T037 [US1] Implement ten L3 storage models in `crates/l3/src/{relational_table,columnar_table,document_store,fulltext_search,vector_search,spatial_search,kv_store,timeseries,object_storage,log_stream}.rs` with default layouts from [type-inventory.md](contracts/type-inventory.md); `relational_table`/`columnar_table`/`vector_search`/`spatial_search`/`timeseries` are schema-**Required**; `document_store`/`fulltext_search`/`log_stream` Optional; `kv_store`/`object_storage` Free
- [ ] T038 [US1] Register L0+L2+L3 descriptors and implementations through `crates/typeset/src/lib.rs` so option-free create succeeds for every creatable type registered so far (FR-029)
- [ ] T039 [US1] Implement container create / describe / alter / drop against `ContainerCatalog` + `CatalogStore` in `crates/types/src/catalog.rs` (and node wiring): type fixed for life; drop refused when dependants exist without cascade (FR-015–FR-019); description never includes key material (FR-017)
- [ ] T040 [US1] Enforce schema ownership on write/read for schema-required/optional types in `crates/types/src/schema.rs` + each L3 impl; additive live evolution without data rewrite; incompatible changes refused with transform hint (FR-017a, FR-017b)
- [ ] T041 [US1] Wire create/describe/drop and type-specific ops through `crates/types/src/datatype.rs` so at least one `002` protocol path exercises each of the ten L3 models; delete in-memory-only stub bodies for the six continuity names under `crates/types/src/` (or the `002` stub module) when real impls are registered
- [ ] T042 [US1] Expose container type name on every listing path used by protocols (FR-020) in `crates/types/src/datatype.rs` and `crates/node` list APIs

**Checkpoint**: Ten L3 models create/describe/write/read/op/drop on a single node; invalid defs allocate nothing; schema evolution matrix green for US1 cases.

---

## Phase 4: User Story 2 - Discover the type inventory and every type's capabilities (Priority: P1)

**Goal**: One queryable catalog lists all 55 types with complete descriptors, plus encodings, codecs, encryption algorithms, and 13 shared capabilities; admin/CLI surfaces and docs match the catalog; starter examples run verbatim.

**Independent Test**: Request catalog via admin surfaces; every inventory name appears once at its level with a complete FR-009 descriptor; compare two nodes of the same release (SC-001 / US2 Independent Test).

### Tests for User Story 2 ⚠️

- [ ] T043 [P] [US2] Add `crates/conformance/tests/catalog_complete.rs` asserting 100% of FR-003–FR-007 names at stated levels with every FR-009 field present (SC-001)
- [ ] T044 [P] [US2] Add starter-example verbatim runs for every descriptor in `crates/conformance/tests/starter_examples.rs` (SC-003)
- [ ] T045 [P] [US2] Add encoding/compression/encryption/capability list assertions in `crates/conformance/tests/catalog_complete.rs` (Gorilla/Delta/Dictionary/RLE; Snappy/ZSTD/LZ4; AES-256-GCM default + ChaCha20-Poly1305; 13 L1 capabilities — US2 scenarios 3–4)

### Implementation for User Story 2

- [ ] T046 [P] [US2] Complete remaining L4 composition **descriptors** (implementations may stub until US5) in `crates/l4/src/{union,federated,materialized_view,distributed,partitioned,replicated,sharded}.rs` so the catalog lists all seven kinds with write-rule metadata and L1 capability references (FR-007, FR-008)
- [ ] T047 [US2] Ensure `typeset` registers exactly 55 types (20 L0 + 18 L2 + 10 L3 + 7 L4) with unique `snake_case` names and derived creatability in `crates/typeset/src/lib.rs` (FR-003–FR-007, FR-014)
- [ ] T048 [P] [US2] Add admin DTOs and ops `types` / `type` / `codecs` / `capabilities` / `catalog` in `crates/admin-proto` per [type-catalog.md](contracts/type-catalog.md) and [config-directives.md](contracts/config-directives.md)
- [ ] T049 [P] [US2] Add CLI commands `types`, `type`, `codecs`, `capabilities`, `catalog`, `containers`, `describe` in `crates/spacestorage` matching [quickstart.md](quickstart.md) §1
- [ ] T050 [US2] Publish encoding/codec/algorithm/capability registries through `crates/types/src/catalog.rs` and the admin/CLI surfaces so drivers can enumerate container-creatable types at startup with no other source (FR-010, FR-012)
- [ ] T051 [US2] Generate or author `docs/types/<name>.md` for every catalog item and keep them consistent with the live catalog (FR-013); document that same-release catalogs are byte-identical and different releases report per-node diffs (FR-011; cluster aggregation may be `#[ignore]` pending `06`)
- [ ] T052 [US2] Embed a valid `starter_example` on every creatable descriptor and validate it at registration (`StarterExampleInvalid`) in `crates/types/src/catalog.rs` (FR-013, FR-045)

**Checkpoint**: `spacestorage types` shows 55 entries; `spacestorage type <name>` returns a complete descriptor; SC-001/SC-003 harness passes for registered claims.

---

## Phase 5: User Story 3 - Configure foundation layout, encoding, compression, and encryption (Priority: P2)

**Goal**: Per-container mode/layout/encoding/compression/encryption validated and applied; restart restores persistent/hybrid intact and memory-mode empty with honest description; forward-only codec/key changes.

**Independent Test**: Same type in memory/persistent/hybrid; each encoding on an applicable kind; each codec; two algorithm/key pairs; restart; invalid combinations refused before allocation (US3 Independent Test / SC-004 / SC-011).

### Tests for User Story 3 ⚠️

- [ ] T053 [P] [US3] Add `crates/conformance/tests/matrix_modes_codecs.rs` covering every creatable type × declared mode × applicable encoding × codec × two algorithm/key pairs with lossless write-read (SC-004)
- [ ] T054 [P] [US3] Add `crates/conformance/tests/restart_restore.rs`: persistent/hybrid intact after restart; unreplicated memory-mode empty with description stating content did not survive (FR-023)
- [ ] T055 [P] [US3] Add `crates/conformance/tests/encryption_scope.rs`: no key material in describe/logs/config; scope `drives` default; memory-mode + `drives` → `EncryptionScopeInvalid`; `drives_and_memory` plaintext scan of stored regions outside transient buffers (SC-011, FR-027a)
- [ ] T056 [P] [US3] Extend `crates/conformance/tests/invalid_definitions.rs` for `UnsupportedStorageMode`, `EncodingNotApplicable`, `KeyUnresolvable` with zero allocation (US3 scenarios 4–5, 7)

### Implementation for User Story 3

- [ ] T057 [US3] Apply per-type hybrid policies and mode support checks in `crates/types/src/validate.rs` + descriptors so unsupported mode fails naming primitive and supported set (FR-021, FR-022)
- [ ] T058 [US3] Wire definition options `mode`, `layout.*`, `encoding.*`, `compression`, `encryption.*` through create/alter in `crates/types/src/definition.rs` and the block pipeline in `crates/storage/src/block.rs` (compression always before encryption)
- [ ] T059 [US3] Enforce encryption scope rules in `crates/types/src/validate.rs` and `crates/crypto`: default `drives`; memory-mode requires `drives_and_memory`; unresolvable key refuses create; at restart unresolvable key → present but unavailable, no plaintext (FR-027, FR-027a)
- [ ] T060 [US3] Implement forward-only compression/encoding/key changes listing all present codecs/keys in describe until transform rewrite (FR-028) in `crates/types/src/catalog.rs` alter path
- [ ] T061 [US3] Complete boot restore orchestration in `crates/storage/src/restore.rs` + `crates/node` so memory mode is documented/described as a volatile tier (FR-023)
- [ ] T062 [US3] Mark defaults vs explicitly set options in container `defaults_mask` in `crates/types/src/catalog.rs` and describe output (FR-017, FR-029)

**Checkpoint**: Mode/codec/encryption matrix and restart/encryption-scope conformance suites pass.

---

## Phase 6: User Story 4 - Apply shared (L1) capabilities to a compatible container (Priority: P2)

**Goal**: Thirteen shared capabilities are declarable on compatible types at any level; incompatible/conflicting declarations refused; accepted declarations handed unchanged to `PlacementDirector`; fixed-after-create params refused with transform hint.

**Independent Test**: Placement stub records declarations; declare each capability on compatible and incompatible types (US4 Independent Test / SC-006).

### Tests for User Story 4 ⚠️

- [ ] T063 [P] [US4] Add `crates/conformance/tests/capability_matrix.rs` exercising every (type, capability) pair: compatible accepted and recorded; incompatible → `CapabilityUnsupported` listing alternatives (SC-006)
- [ ] T064 [P] [US4] Add fixed-after-create and conflict cases in `crates/conformance/tests/capability_matrix.rs` (`sharding.key` / `partitioning.scheme|key` fixed; conflicting declarations → `CapabilityConflict`)

### Implementation for User Story 4

- [ ] T065 [US4] Populate the 13 capability descriptors (`replication`, `sharding`, `partitioning`, `node_placement`, `failure_handling`, `rebalancing`, `consistency`, `persistent_placement`, `labels`, `distributed_transactions`, `consensus`, `leader_election`, `quorum`) with declarable/fixed params from [shared-capabilities.md](contracts/shared-capabilities.md) in `crates/placement/src/lib.rs`
- [ ] T066 [US4] Fill every type's `CapabilityMatrixRow` in `crates/l0/src/`, `crates/l2/src/`, `crates/l3/src/`, `crates/l4/src/` descriptors; newly registered capabilities default to `Unsupported` for existing types (FR-031, US4 scenario 7)
- [ ] T067 [US4] Validate capability declarations before placement and forward `PlacementInfo` via `PlacementDirector` in `crates/types/src/validate.rs` + `crates/placement/src/director.rs`; container-as-one-unit — reject per-component placement (FR-032, FR-033)
- [ ] T068 [US4] Describe undeclared capabilities as placement defaults named by the director (FR-034) in `crates/types/src/catalog.rs` describe path; alter changeable params; refuse fixed params with transform hint (FR-035) in `crates/types/src/validate.rs`
- [ ] T069 [US4] Confirm L2 `ordered_map` in `crates/l2/src/ordered_map.rs` accepts replication under the same declaration form as L3 in matrix rows and describe output (FR-030)

**Checkpoint**: SC-006 green; local director records accepted declarations only.

---

## Phase 7: User Story 5 - Compose storage objects across levels (L4) (Priority: P2)

**Goal**: Seven L4 composition kinds create/read/route (or refuse writes) per kind rules; acyclic; same-namespace; nesting depth ≤ 8; cascade/drop semantics; listed like any container.

**Independent Test**: Compose each L4 kind over members; read; drop with/without cascade; refuse cyclic and cross-namespace (US5 Independent Test / SC-009).

### Tests for User Story 5 ⚠️

- [ ] T070 [P] [US5] Add `crates/conformance/tests/compositions.rs` for all seven kinds: union/MV read-only writes refused; federated/partitioned/sharded/replicated/distributed single-member routing; ambiguous write refused with nothing stored; cycles and cross-namespace refused; depth > 8 → `CompositionDepthExceeded{limit=8}` (SC-009, FR-008a)
- [ ] T071 [P] [US5] Add cascade vs non-cascade member drop and missing-member policy cases in `crates/conformance/tests/compositions.rs` (FR-019)

### Implementation for User Story 5

- [ ] T072 [P] [US5] Implement `union` (read-only; union-compatibility table from [type-inventory.md](contracts/type-inventory.md)) and `federated` (routed writes) in `crates/l4/src/{union,federated}.rs`
- [ ] T073 [P] [US5] Implement `materialized_view` with sync/async freshness, refresh state, and read-only direct writes in `crates/l4/src/materialized_view.rs`
- [ ] T074 [P] [US5] Implement `distributed`, `partitioned`, `replicated`, `sharded` as compositions of named L1 capabilities (no new placement capabilities) in `crates/l4/src/{distributed,partitioned,replicated,sharded}.rs` (FR-008)
- [ ] T075 [US5] Resolve members to `ContainerId`s, enforce same-namespace, acyclicity, depth ≤ **8**, and kind member-count minima in `crates/types/src/validate.rs` ([composition.md](contracts/composition.md))
- [ ] T076 [US5] Compute composition operation sets from members in `crates/l4/src/lib.rs`; answer multi-member atomic write with pointer to L1 `distributed_transactions` (FR-008a) in `crates/types/src/ops.rs`; list compositions with type name on every protocol listing via `crates/types/src/datatype.rs` (FR-020)
- [ ] T077 [US5] Register L4 implementations in `crates/typeset/src/lib.rs` and apply shared-capability validation to composition kinds like any other type (US5 scenario 10)

**Checkpoint**: SC-009 green; compositions appear in namespace listings.

---

## Phase 8: User Story 6 - Drivers and the planner see every type through one abstract interface (Priority: P3)

**Goal**: Every catalog operation is reachable only through the abstract interface; capability queries are metadata-only; canonical round-trips are lossless; per-type stats are labelled for `08`/`07`.

**Independent Test**: Invoke every catalog operation via the abstract interface; query capabilities without data; round-trip canonical values; ask for unsupported ops (US6 Independent Test / SC-007 / SC-008 / SC-012).

### Tests for User Story 6 ⚠️

- [ ] T078 [P] [US6] Add `crates/conformance/tests/operations_reachable.rs` invoking every catalog-claimed operation through the abstract interface only; unsupported ops name type and operation; capability queries touch no container data (SC-007, FR-049)
- [ ] T079 [P] [US6] Add `crates/conformance/tests/canonical_all_types.rs` lossless round-trips and accepted earlier versions for every type (SC-008)
- [ ] T080 [P] [US6] Add `crates/conformance/tests/type_stats.rs` asserting access/hit-miss/duration labelled with type and namespace (SC-012)

### Implementation for User Story 6

- [ ] T081 [US6] Complete capability descriptors on every type (`name`, inputs, outputs, ordering, filtering, `native`) answered from registry in `crates/types/src/ops.rs` (FR-039)
- [ ] T082 [US6] Extend canonical payload schemas for every type in `crates/types/src/canonical.rs` per [canonical-payloads.md](contracts/canonical-payloads.md) (emit current version; accept documented earlier versions — FR-042)
- [ ] T083 [US6] Ensure no type-private access path is required by drivers/planner; all create/describe/alter/list/drop/catalog/ops go through `crates/types/src/datatype.rs` (FR-040)
- [ ] T084 [US6] Hook stats counters into every container access path in `crates/types/src/stats.rs` and expose to `crates/node` stats for `08` (FR-044)
- [ ] T085 [US6] Re-run the `002` cross-protocol matrix in `crates/conformance/tests/` against the full creatable inventory (50 types) and fix any mapping gaps in handler crates without renaming traits

**Checkpoint**: SC-007/SC-008/SC-012 green; FR-049 harness fails CI if any catalog claim is overstated.

---

## Phase 9: User Story 7 - Extend the inventory without disturbing existing types (Priority: P3)

**Goal**: Registration of new types/encodings/codecs/algorithms/capabilities is a defined act with complete descriptors; incomplete/multi-level registrations refused; deprecation blocks new creates but not existing data; removal blocked while containers exist.

**Independent Test**: Register test type/encoding/codec; reject incomplete/multi-level; create with new items; deprecate; confirm existing untouched (US7 Independent Test / SC-010).

### Tests for User Story 7 ⚠️

- [ ] T086 [P] [US7] Add `crates/conformance/tests/registration_isolation.rs`: after registering test type/encoding/codec/algorithm/capability, pre-existing descriptors and container descriptions are byte-for-byte unchanged; new item immediately selectable (SC-010, FR-046)
- [ ] T087 [P] [US7] Add registration failure cases for `IncompleteDescriptor` and `MultipleLevels` and deprecation/removal rules in `crates/conformance/tests/registration_isolation.rs` (FR-045, FR-047, FR-048)

### Implementation for User Story 7

- [ ] T088 [US7] Harden `TypeSystem::builder()` registration validation in `crates/types/src/catalog.rs` / `crates/typeset/src/lib.rs` (complete descriptor, exactly one level, duplicate name, layout ⊆ composed_from, default codec listed, hybrid policy present when hybrid supported)
- [ ] T089 [US7] Implement encoding/codec/algorithm/capability registration paths that do not mutate existing descriptors in `crates/codec/src/lib.rs`, `crates/crypto/src/lib.rs`, `crates/placement/src/lib.rs` (FR-046)
- [ ] T090 [US7] Implement deprecation (`TypeDeprecated` on new creates; existing fully usable; catalog shows status) and removal refusal listing live containers in `crates/types/src/catalog.rs` (FR-047, FR-048)
- [ ] T091 [US7] On new shared-capability registration, leave every existing type `Unsupported` until its descriptor is updated (FR-031 / US4 scenario 7) in `crates/placement/src/matrix.rs`

**Checkpoint**: SC-010 green; extension path usable by maintainers without touching existing containers.

---

## Phase 10: Polish & Cross-Cutting Concerns

**Purpose**: Docs, fixtures, performance gates, and end-to-end quickstart validation across stories.

- [ ] T092 [P] Copy remaining fixtures from `specs/003-type-system/contracts/fixtures/` (containers `*.def`, `invalid/*` per validation code) into conformance resources and keep `docs/examples/node-typed.conf` in sync
- [ ] T093 [P] Finish `docs/storage.md` (modes, codecs, encryption scopes, restore, volatile memory tier) and ensure `docs/types/` covers every catalog item
- [ ] T094 Run [quickstart.md](quickstart.md) end-to-end on a fresh node using `docs/examples/node-typed.conf` and fix gaps until every mapped success criterion is demonstrated
- [ ] T095 Measure and document catalog lookup &lt; 1 ms, create/describe/drop &lt; 50 ms p95, additive schema change &lt; 100 ms, boot restore of 10 000 definitions &lt; 30 s (plan Performance Goals) in `docs/storage.md` and/or `crates/types/benches/`
- [ ] T096 [P] Add CI job in `.gitlab-ci.yml` (or workspace CI config) that builds `002` handler crates against `crates/types` each commit to catch non-additive trait changes
- [ ] T097 Mark cluster catalog divergence aggregation test `#[ignore]` with reason pending `06` in `crates/conformance/tests/catalog_complete.rs` while keeping per-node digest reporting (FR-011)
- [ ] T098 Code cleanup across `crates/storage/src/` and `crates/node`: ensure Tokio worker threads never block on file I/O (`tokio::fs`; compaction/ANN/dictionary/re-encrypt on bounded `spawn_blocking`); keep `cargo deny` clean

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Setup — **BLOCKS** all user stories
- **User Story 1 (Phase 3)**: After Foundational — MVP; needs engines + L0/L2/L3 implementations
- **User Story 2 (Phase 4)**: After Foundational; practically after US1 descriptors exist for L3, completes full 55-name catalog + admin/CLI
- **User Story 3 (Phase 5)**: After US1 (containers to configure); deepens storage/crypto paths
- **User Story 4 (Phase 6)**: After US1 (containers to declare on); uses placement matrix
- **User Story 5 (Phase 7)**: After US1 + US4 (members + capability rules for distribution kinds)
- **User Story 6 (Phase 8)**: After US1 + US2 (full inventory + abstract interface claims)
- **User Story 7 (Phase 9)**: After US2 (registration surface exists)
- **Polish (Phase 10)**: After desired stories complete

### User Story Dependencies

- **US1 (P1)**: No dependency on other stories — MVP
- **US2 (P1)**: Independently testable (catalog read with no data); shares foundational registries with US1
- **US3 (P2)**: Builds on US1 containers; independently testable via mode/codec matrix
- **US4 (P2)**: Builds on US1; independently testable with local `PlacementDirector`
- **US5 (P2)**: Needs US1 members; distribution kinds need US4 declaration model
- **US6 (P3)**: Needs US1+US2 inventory and ops claims
- **US7 (P3)**: Needs US2 registration/catalog surface

### Within Each User Story

- Tests (where included) MUST be written and FAIL before implementation
- Descriptors/models before services/engines
- Validation before allocation
- Story complete before moving to next priority when staffing is serial

### Parallel Opportunities

- Phase 1: T001–T005, T007–T008 marked [P]
- Phase 2: identity/error/datatype/ops/canonical/stats (T011–T012, T017–T020) in parallel after descriptor skeleton; codec/crypto can proceed in parallel with types core once interfaces settle
- Once Foundational completes: US2 admin/CLI surfaces can proceed in parallel with US1 L0/L2 file work if descriptors are stubbed carefully (prefer serial US1→US2 if FR-049 would over-claim)
- Within a story: all [P] test tasks; parallel L0/L2/L4 source files on different paths
- US4 and US3 can proceed in parallel after US1 if staffing allows

---

## Parallel Example: User Story 1

```bash
# Launch US1 tests together (fail first):
Task: "SC-002 L3 smoke in crates/conformance/tests/starter_examples.rs"
Task: "Invalid definitions in crates/conformance/tests/invalid_definitions.rs"
Task: "Schema evolution in crates/conformance/tests/schema_evolution.rs"

# Launch independent L0/L2 modules together:
Task: "L0 hash_table/bplus_tree/... in crates/l0/src/"
Task: "L2 kv_collection/document/... in crates/l2/src/"
```

---

## Parallel Example: User Story 2

```bash
Task: "catalog_complete.rs SC-001 assertions"
Task: "starter_examples.rs SC-003"
Task: "admin-proto catalog DTOs"
Task: "CLI types/type/codecs/capabilities/catalog commands"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL)
3. Complete Phase 3: User Story 1 (ten L3 models)
4. **STOP and VALIDATE**: US1 Independent Test / SC-002
5. Demo a multiparadigm namespace on one protocol

### Incremental Delivery

1. Setup + Foundational → engine and catalog seams ready
2. US1 → typed L3 containers (MVP)
3. US2 → full discoverable catalog + admin/CLI
4. US3 → durable/configurable storage characteristics
5. US4 → validated L1 declarations
6. US5 → L4 composition
7. US6 → driver/planner contract proven
8. US7 → governed extension path
9. Polish → docs, quickstart, perf, CI guards

### Parallel Team Strategy

1. Team completes Setup + Foundational together
2. After Foundational:
   - Dev A: US1 L0/L2/L3 implementations
   - Dev B: US2 admin/CLI + catalog completeness (coordinate FR-049 claims)
   - Dev C: codec/crypto property tests and storage restore hardening (feeds US3)
3. Then US3/US4 in parallel; US5 after US4; US6/US7 last

---

## Notes

- [P] = different files, no dependencies on incomplete tasks
- [Story] labels map to spec user stories US1–US7
- Tests included because spec SC-001–SC-012 and FR-049 explicitly require the conformance harness
- Do not over-claim in descriptors: Preview types (e.g. `scann`) must under-claim until oracles pass
- Interim seams only: `keyring_file`, single-node `PlacementDirector`, node-local `CatalogStore`
- Commit after each task or logical group; stop at any checkpoint to validate independently
