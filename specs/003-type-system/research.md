# Phase 0 Research: Multiparadigm Type System (L0–L4)

**Feature**: `003-type-system` | **Date**: 2026-09-13 | **Plan**: [plan.md](plan.md)

Every Technical Context item is resolved below. No `NEEDS CLARIFICATION` remains. Inputs: the feature spec (with its five clarifications), `.specify/memory/constitution.md`, and the shipped contracts of `001` (runtime, config grammar, buffers, admin) and `002` (abstract datatype interface, canonical representation, execution boundary, config directives).

---

## R1. Where the type system lives in the workspace

**Decision**: `crates/types` keeps its name, its crate identity and the trait signatures `002` depends on, and becomes the real implementation. Four engine crates go **below** it (`codec`, `crypto`, `storage`, `placement`) and four type libraries go **beside** it (`l0`, `l2`, `l3`, `l4`), assembled by `typeset`.

**Rationale**: `002`'s eight handler crates depend on `spacestorage-types` and its contract states that `03` replaces the inventory *behind the same traits*. Renaming or relocating the crate would force edits in every handler and break the dependency-graph enforcement that `002` relies on for FR-012. Splitting engines out keeps the block pipeline (encode → compress → encrypt) in one place for all 50 creatable types instead of per-type forks, and splitting type libraries by level makes the architecture a compile-time property: `l3` may depend on `l2` and `l0`, never the reverse, and `l4` depends on neither (it composes through `ContainerRef`).

**Alternatives considered**: one giant `types` crate (rejected: 60–80 k lines in one compilation unit, and nothing stops an L0 primitive from importing an L3 model); a crate per type (rejected: 55 crates, unusable build times, and the descriptor boilerplate dominates); keeping `002`'s interim types alongside the real ones (rejected: two `relational_table`s cannot both be registered — FR-046 duplicate check — and the point of this feature is that data starts surviving restarts).

## R2. How the abstract datatype interface grows without breaking `002`

**Decision**: additive growth only. New capability (schema access, descriptor lookup, layout introspection, capability descriptors, storage-mode queries) arrives as **default-bodied** trait methods on `Datatype`/`Container`, plus new registry entries and new `TypeError` variants. No existing method is renamed, removed or given a new signature. A CI job compiles `002`'s handler crates against every commit of `crates/types`.

**Rationale**: `002`'s contract says exactly this ("`03` may add methods with default implementations … MUST NOT remove or rename"). Default bodies mean an L0 primitive that has no schema simply inherits `fn schema(&self) -> Option<&ContainerSchema> { None }`.

**Alternatives considered**: a v2 trait with a blanket adapter (rejected: two traits in the registry, and drivers would have to pick); a breaking change with a coordinated edit of all handlers (rejected: the contract forbids it and it would make `002` and `003` un-mergeable independently).

## R3. Type name vocabulary

**Decision**: machine names are lowercase `snake_case`, stable, and unique across all levels. The 55 names are fixed in [contracts/type-inventory.md](contracts/type-inventory.md). The six names `002` already uses (`kv_collection`, `relational_table`, `document_store`, `object_collection`, `vector_collection`, `ordered_map`) are kept exactly, so `002`'s canonical `$type` strings and per-protocol mapping tables survive unchanged. Display names in the catalog carry the spec's spelling (`B+tree`, `Radix Tree / Patricia Trie`, `Stack / Queue / Deque`).

Two inventory entries needed an interpretation to be creatable containers:

- **`deque`** covers the spec's single `Stack / Queue / Deque` entry: one type whose operation set includes both ends plus `push`/`pop`/`peek`, with `stack` and `queue` as documented usage patterns rather than separate types (the intent lists them on one line).
- **`field_path`** is a path → value store over a radix tree: keys are dotted/bracketed paths, values are canonical values, and prefix navigation is a native operation. It is usable standalone (attribute or configuration store) and as the component an L3 document model uses to resolve paths.

**Rationale**: the clarified FR-014 makes every L2 abstraction creatable, so every entry needs a coherent standalone meaning; inventing pseudo-types or marking some L2 entries non-creatable would contradict the clarification. Keeping `002`'s six names avoids a canonical-representation version bump on day one.

**Alternatives considered**: splitting `stack`, `queue`, `deque` into three types (rejected: the intent lists one item and three types would triple the descriptor and conformance surface for one data structure); treating `field_path` as a schema concept only (rejected: it is listed as an L2 abstraction and the clarification makes L2 creatable).

## R4. Type descriptor schema

**Decision**: one `TypeDescriptor` record with every field FR-009 lists, all mandatory at registration: `name`, `display_name`, `level`, `kind`, `storage_modes { supported, default, hybrid_policy }`, `operations[]` (capability descriptors, R18), `capabilities[]` (per shared capability: unsupported, or supported with declarable and fixed-after-create parameters), `composed_from[]` + `default_layout`, `encodings[]` / `codecs[]` / `encryption` applicability, `creatable`, `schema_kind` (required | optional | free), `canonical { current_version, accepted_versions[] }`, `deprecation`, `starter_example`. Registration rejects a descriptor missing any field or naming zero or more than one level.

**Rationale**: FR-009 and FR-045 make completeness a validation rule, not a convention; a struct with non-`Option` fields plus a builder that cannot be finished early turns most of FR-045 into a compile error, and the remainder (level count, example validity) into two registration checks.

**Alternatives considered**: a loosely typed map of attributes (rejected: FR-045's "reject naming the missing fields" becomes a runtime string-matching exercise and IDE support disappears); optional fields with defaults (rejected: a defaulted `operations[]` is exactly the overstated claim FR-049 calls a defect).

## R5. Registration mechanism for new inventory items

**Decision**: explicit registration through a builder — `TypeSystem::builder().register_type(...)`, `.register_encoding(...)`, `.register_codec(...)`, `.register_algorithm(...)`, `.register_capability(...)` — called once in `crates/typeset`. No link-time collection (`inventory`/`linkme`), no dynamic loading. Registration is fallible (`DuplicateType`, `IncompleteDescriptor`, `MultipleLevels`, `UnknownCapability`) and the resulting `TypeSystem` is immutable and `Arc`-shared.

**Rationale**: FR-046 requires that registering an item changes nothing existing — a property that is easy to test when registration is one ordered list in one file, and hard to reason about when a distributed `#[distributed_slice]` decides order at link time. An immutable system after build also gives the catalog its FR-011 "identical per release" property for free.

**Alternatives considered**: `inventory`-style auto-registration (rejected: hidden ordering, and a type registers itself merely by being linked, which makes the "new capability defaults to incompatible" rule of FR-031 harder to audit); dynamic plug-ins (rejected: Principle III, one process, one binary — and a dynamic type could not be verified by the FR-049 harness at build time).

## R6. Container identity

**Decision**: every container gets a `ContainerId` = UUID v7 at creation, immutable for its lifetime. Names are unique per namespace and mutable; compositions reference members by `ContainerId`, never by name. The catalog indexes both `(namespace, name) → id` and `id → definition`.

**Rationale**: the spec's edge case "composition member renamed: compositions reference members by identity, not by name" makes this mandatory. UUID v7 is time-ordered, which keeps catalog log locality and gives a natural creation ordering without a separate column.

**Alternatives considered**: name-based references with rename cascade (rejected by the spec edge case); monotonic u64 per node (rejected: collides when `06` merges per-node catalogs into a cluster catalog).

## R7. Catalog store format and boot restore

**Decision**: node-local `storage.data_dir/catalog/` holds an append-only `catalog.log` of definition mutations (create, alter, schema change, capability change, drop, compose) plus periodic `catalog.snapshot.<n>` files; each record is length-prefixed, CRC32-checked and fsynced before the operation is acknowledged. At boot the newest valid snapshot is loaded and the log is replayed past it; then per-container content restore runs by mode (R8). The whole path sits behind a `CatalogStore` trait so `06` can front it with a controller.

**Rationale**: FR-023 and Principle XII require definitions and persistent content to come back without operator reconstruction, and FR-018 requires validation before allocation — both are simplest when the definition is durable *before* any data directory is created. A log plus snapshot is the same shape as the WAL the storage engine already needs, so the framing and checksum code is shared.

**Alternatives considered**: storing definitions inside each container's own WAL (rejected: listing a namespace would require opening every container, and a corrupt container would hide its own existence); an embedded KV library for metadata (rejected: Principle I plus a second storage engine inside a storage product).

## R8. Storage modes and hybrid policy

**Decision**: three modes as the spec defines them, chosen per container, validated against the primitive's declared support.

- `memory`: all content in the `types.memory` buffer; **volatile** per Clarification Q2 — at boot the definition and options return, the content does not, and the description says so. Replication may repopulate it (`04`).
- `persistent`: content durable on drives; memory used only as cache (`storage.block_cache`).
- `hybrid`: a declared in-memory portion plus a persistent remainder, governed by a per-type `HybridPolicy` in the descriptor — one of `recent { window }` (time-series segments, log stream), `hot_set { fraction | bytes }` (K/V, map, document), `write_buffer` (LSM-backed models where the memtable is the memory portion), or `index_resident` (ANN and index types that keep the graph or dictionary in memory and payloads on drives).

All three modes present the same operation set for a given type (FR-022).

**Rationale**: making the policy a descriptor field with four named shapes keeps hybrid honest and documentable per FR-022 instead of "some of it is cached"; `write_buffer` is what an LSM already does, so most L3 models get hybrid for free.

**Alternatives considered**: a free-form policy expression per container (rejected: unvalidatable and undocumentable per type); hybrid as an automatic tiering heuristic (rejected: the spec requires the description to state which part is where).

## R9. Block pipeline and header

**Decision**: one pipeline for every primitive: values → **encoding** (per field or data kind) → block assembly (target 64 KiB, configurable) → **compression** (per block) → **encryption** (per block, AEAD) → storage or memory. Compression always precedes encryption (FR-026) and cannot be reversed by configuration. Every block carries a fixed header: magic, format version, `encoding_id`, `codec_id`, `uncompressed_len`, `crc32` of the *pre-encryption* payload, and when encrypted `algorithm_id`, `key_id`, 96-bit nonce, with the block coordinates (container id, file id, block index) as AEAD associated data.

**Rationale**: encryption last is the only order that preserves compression ratio; AAD binding to coordinates prevents a block from being replayed at another offset or in another container; keeping the CRC over the plaintext payload means corruption is distinguishable from a wrong key.

**Alternatives considered**: whole-file encryption (rejected: breaks random block reads and the hybrid memory portion); nonce derived from a counter only (rejected: counter reuse after a crash — nonce = random 96-bit with a per-key block budget, rotated by key version).

## R10. Compression codecs

**Decision**: `none`, `snappy` (`snap`), `zstd` (`structured-zstd`, levels 1–11 exposed, default 3), `lz4` (`lz4_flex`, already in `002`). The registry is open (FR-046). Default codec per type is in the descriptor; the global default is `zstd` for persistent modes and `none` for memory mode.

**Rationale**: FR-024 names Snappy, ZSTD and LZ4 as mandatory. Principle I forbids `zstd-sys`; `structured-zstd` is a pure-Rust, no-FFI fork of `ruzstd` with the full level range and frames that upstream C zstd can decode, which also keeps the door open to interoperating with ClickHouse and Cassandra clients that `002` serves. `none` for memory mode avoids paying CPU for data that is already in RAM.

**Risk and mitigation**: `structured-zstd` is young. Property tests compress with it and decompress with the independent `ruzstd` decoder (and vice versa), the level range is pinned in the registry, and `CompressionRegistry` makes the implementation swappable without touching a type.

**Alternatives considered**: `zstd`/`zstd-sys` (rejected: Principle I, explicitly, as `002` already rejected C-backed crates); shipping only Snappy and LZ4 and marking ZSTD "future" (rejected: FR-024 makes it mandatory); `brotli` as a substitute (rejected: not in the required list, and the pure-Rust crate is slower at comparable ratios for block-sized inputs).

## R11. Encodings and applicability

**Decision**: `plain`, `gorilla`, `delta`, `dictionary`, `rle`, each declaring the **data kinds** it applies to; encodings are chosen per field or per data kind within one container (FR-025) and an encoding applied to an inapplicable kind is rejected with the applicable list (spec Story 3 scenario 5).

| Encoding | Applies to | Notes |
|---|---|---|
| `plain` | every kind | always applicable; the fallback default |
| `gorilla` | `f32`, `f64`, and `timestamp` streams | XOR + leading/trailing-zero bitstream, the Facebook Gorilla scheme |
| `delta` | integers, `timestamp`, `date`, ordered keys | delta-of-delta with zigzag varint |
| `dictionary` | `string`, `bytes`, `enum`, low-cardinality scalars | per-block dictionary; on cardinality overflow a block falls back to `plain` and records it (spec edge case), never refuses the write |
| `rle` | `bool`, `enum`, integers, any kind with long runs | run-length over the post-encoding symbol stream |

**Rationale**: the four required encodings map cleanly onto data kinds, and the spec's edge case about dictionary cardinality demands a documented per-block fallback rather than an error.

**Alternatives considered**: encoding chosen automatically per block by sampling (rejected: FR-025 requires a selectable, describable choice — automatic selection may be added later as an explicit `auto` encoding); frame-of-reference and bit-packing as separate encodings (rejected: not in the required list; they are implementation details inside `delta`).

## R12. Encryption algorithms and key seam

**Decision**: `aes-256-gcm` (default) and `chacha20-poly1305`, both RustCrypto, selected per container alongside a `KeyRef` and a scope. A `KeyAuthority` trait resolves `KeyRef → DataKey` with caching and version awareness; the interim provider is a `0600` keyring file (`keys { keyring_file … }`) that `07` replaces. Container keys are derived per container and per key version with HKDF-SHA-256 from the referenced key material, so a rotation adds a version rather than rewriting blocks. Key material lives only in `crates/crypto` inside a `Zeroizing` newtype with no `Debug`/`Display`/`Serialize`.

**Rationale**: Principle XIII requires per-container encryption now, and `07` owns key management; `002` set the precedent of an interim file-backed provider behind the exact trait the later feature implements. HKDF derivation is what makes FR-028's "old data readable under the old key, new data under the new" true without a rewrite.

**Alternatives considered**: a single algorithm (rejected: FR-024 says "more than one algorithm", and ChaCha20 matters on hardware without AES-NI); encrypting with the referenced key directly (rejected: key rotation would then require rewriting every block, contradicting FR-028); XTS for block storage (rejected: no authentication, and AEAD with coordinate AAD gives tamper detection).

## R13. Encryption scope implementation

**Decision**: scope `drives` (default) encrypts blocks on their way to disk only. Scope `drives and memory` additionally keeps memtable entries, block-cache entries and the whole content of a memory-mode container encrypted at rest in RAM, decrypting per access into a short-lived buffer. The exempt "transient per-operation buffer" is defined precisely in the contract: the decrypted plaintext of the blocks touched by a single in-flight operation, plus the values held in the response being serialised, all dropped (and zeroized) before the operation completes. A memory-mode container that declares encryption must declare `drives and memory`.

**Rationale**: the clarification fixed the default and the memory-mode rule; what remained was where to draw the exemption line, and drawing it at "one in-flight operation" is both implementable and testable — SC-011 scans memory-resident regions for plaintext outside those buffers.

**Alternatives considered**: encrypting the entire process heap (rejected: not implementable in a Rust monolith without an allocator rewrite, and it would make the guarantee unverifiable); leaving the exemption undefined (rejected: the spec's own assumption says the boundary must be documented so the guarantee is not overstated).

## R14. Schema model

**Decision**: a container schema is an ordered list of fields plus optional key definitions and type-specific parameters. Field = `name`, `ValueDomain`, `nullable`, `default`, optional `encoding` override. Value domains: `bool`, `i8|i16|i32|i64`, `u8|u16|u32|u64`, `f32`, `f64`, `decimal(p,s)`, `string(max?)`, `bytes(max?)`, `timestamp(us)`, `date`, `time`, `uuid`, `json`, `vector(f32, dims)`, `geo_point`, `geo_shape`, `enum{…}`, `array<T>`, `map<K,V>`, `struct{…}`. Each type declares `schema_kind`: **required** (`relational_table`, `columnar_table`, `vector_search`, `timeseries`, `spatial_search`), **optional** (`document_store`, `fulltext_search`, `map`-family, `field_index`, most L2), **free** (`kv_store`, `object_storage`, `log_stream`, most L0 data structures).

**Rationale**: Clarification Q4 put schema here, and drivers need a single vocabulary to render as SQL column types, CQL types, Elasticsearch mappings and ClickHouse types. The three schema kinds let `kv_store` stay schemaless while `relational_table` demands columns, which is what protocol semantics already expect.

**Alternatives considered**: reusing the canonical representation's JSON shapes as the schema (rejected: no domains, no nullability, no keys — drivers could not render a `CREATE TABLE`); a per-type ad-hoc options bag (rejected: that is the per-driver schema store FR-017a forbids).

## R15. Additive schema evolution matrix

**Decision**: allowed in place — add a nullable field or column; add a field with a default; widen a domain along a documented lattice (`i8→i16→i32→i64`, `u*→` the next wider signed or unsigned, `f32→f64`, `decimal(p,s)→decimal(p',s)` with `p'>p`, `string(n)→string(m>n)→string`, `bytes` likewise, `enum` value added, non-nullable → nullable); add an index or field path; add a member to an `array<T>`'s element struct under the same rules. Refused (transform required) — drop or rename a field, narrow a domain, change a field's type off the lattice, change nullability to non-nullable, change a key definition, change a vector dimension, change a partition or sharding key. Old data is never rewritten: readers apply the current schema over stored blocks, and a missing field reads as its default or null.

**Rationale**: FR-017b names the two sets; the lattice makes "widening" testable rather than a judgement call, and read-time defaulting is what lets the change complete in constant time on a large container (the performance goal in the plan).

**Alternatives considered**: rewriting blocks on widening (rejected: an additive change would then take hours on a large container and could not be "no data rewrite"); allowing renames via an alias table (rejected: FR-017b lists rename as incompatible, and aliasing hides the change from drivers that cache nothing).

## R16. Shared-capability declarations and the placement seam

**Decision**: a `CapabilityDecl` per capability with typed parameters (`replication { factor, anti_affinity: label, mode: sync|async }`, `sharding { key, shards? }`, `partitioning { scheme: range|hash|time, key, interval? }`, `node_placement { labels }`, `persistent_placement { labels }`, `consistency { mode }`, `quorum { write, read }`, and parameterless markers for `failure_handling`, `rebalancing`, `distributed_transactions`, `consensus`, `leader_election`). The per-type matrix in the descriptor says unsupported, or supported with which parameters are declarable and which are fixed after creation. Validation happens in `crates/placement::matrix` before anything is handed to a `PlacementDirector`; the interim `LocalDirector` reports one replica and bridges to `002`'s `PlacementInfo`. A newly registered capability is unsupported for every existing type until its descriptor says otherwise (FR-031).

**Rationale**: FR-030–FR-036 make this feature the owner of *declaration and compatibility* and `04` the owner of *semantics*; a seam crate with the matrix on our side and the director on theirs is the smallest split that satisfies both, and it reuses the `PlacementInfo` trait `002` already consumes.

**Alternatives considered**: storing declarations as opaque option strings passed to `04` (rejected: FR-032 requires rejection *before* placement, which needs typed parameters here); putting the matrix in `04` (rejected: the matrix is per type, so it belongs with the descriptor, and `04` does not exist yet).

## R17. Composition semantics

**Decision**: an L4 container stores an ordered member list of `ContainerId`s plus a kind-specific rule (union member order; federated routing predicate; view source and freshness policy; partition ranges; shard key; replication targets). Cycles are rejected by a walk at definition time; nesting depth is capped at **8** and stated in the catalog. Write rules follow Clarification Q3 exactly: `union` and `materialized_view` are read-only; `federated`, `partitioned`, `sharded`, `replicated` and `distributed` route each write to the single member the rule resolves, refusing zero-or-many with nothing stored. Missing-member tolerance is per kind: `union` and `federated` continue with the remaining members and record the removal; `partitioned`/`sharded` refuse operations that address the missing member's range; `materialized_view` keeps serving its last refreshed state and reports staleness. Refresh policies: `sync` (the source write completes after the view is updated) and `async { interval | on_commit }` with last-refresh state in the description.

**Rationale**: the clarification fixed writes; the remaining decisions (identity references, depth cap, per-kind tolerance) come straight from the spec's edge cases and needed concrete values to be testable.

**Alternatives considered**: unlimited nesting (rejected: planning cost and cycle-detection cost become unbounded; the spec asks for a documented depth); cascading writes to all members of a `union` (rejected by the clarification).

## R18. Capability descriptors and metadata-only answers

**Decision**: `OperationSpec` grows from `002`'s `{name, args, result, mutates}` to `{name, kind: read|write|admin, inputs, outputs, native: bool, ordering: none|by_key|by_field(list)|by_score, filtering: none|equality|range|prefix|full, cost_class: o1|log_n|scan|index, since_version}`. Capability queries are answered from the descriptor in the registry — a pure in-memory lookup that never opens a container (FR-039) — and an operation outside the set returns `TypeError::NotSupportedByType { type, op }` before any data access (FR-041).

**Rationale**: `05`'s planner needs to know not only *that* an operation exists but what ordering and filtering it guarantees, otherwise it cannot choose between a native range read and a scan; adding those fields now avoids a second descriptor pass when `05` lands. `cost_class` is deliberately coarse — a real cost model belongs to the planner.

**Alternatives considered**: exposing full cost statistics per operation (rejected: that is `05`'s planner input and depends on container contents, which FR-039 forbids touching); keeping `002`'s four-field spec (rejected: `native` and `ordering` are exactly what FR-039 enumerates).

## R19. Canonical representation for 55 types

**Decision**: keep canonical v1 (envelope `{"$d":…,"$type":"<type>/<kind>","$v":1}`, RFC 8785-style canonicalisation) from `002` unchanged, and extend the payload-schema table to every type. `$type` kinds grow with `element` (L0 single-structure containers), `entry`, `segment`, `member` and `composition-meta`. Each type declares `canonical.current_version` and `accepted_versions`, so a future payload change is a per-type version bump rather than a global one.

**Rationale**: FR-042 requires exactly one self-describing versioned representation per type and acceptance of earlier versions; `002`'s vectors and drivers already depend on v1 byte stability, so the extension must be purely additive.

**Alternatives considered**: a binary canonical form (rejected: `002` carries canonical bytes through JSON-shaped carriers such as Elasticsearch `_source`, where a binary form would need base64 and lose byte-identity); per-level representations (rejected: FR-042 says one per type, and drivers would need level-aware parsing).

## R20. ANN, full-text and spatial implementation strategy

**Decision**: implement the index primitives in-house over our own block pipeline, using existing pure-Rust crates as **dev-dependency oracles** rather than runtime dependencies:

- `hnsw` — in-house graph over our storage, with `instant-distance` and `fast-hnsw` used in tests to assert recall within a tolerance of a reference build on the same data.
- `scann` — in-house partitioning + anisotropic quantisation + reordering, with `vicinity`'s experimental ScaNN feature as the recall oracle; sequenced last and allowed to ship as `availability: preview` with an under-claiming descriptor.
- `kd_tree` — `kiddo` for the in-memory structure, our serialization for blocks.
- `spatial_index` / `spatial_search` — `rstar` R-tree in memory, our serialization for blocks.
- `bitmap` / `bitmap_index` — `roaring`, whose portable serialization becomes the block payload.
- `ngram_index` / `fulltext_search` — in-house inverted index (postings in SSTables, dictionary in a radix tree) with `unicode-segmentation` + `unicode-normalization` for tokenisation.

**Rationale**: an L0 primitive must support memory, persistent and hybrid modes and must pass through our encoding, compression and encryption pipeline (FR-021, FR-024). Crates that own their own files or serialization cannot satisfy that, so they are used where they are a pure in-memory data structure with a serialization we control (`kiddo`, `rstar`, `roaring`) and replaced by our own code where they are a storage engine in miniature (HNSW, ScaNN, Tantivy). Keeping the ANN crates as test oracles turns "our HNSW is correct" into a measured number rather than an assertion.

**Alternatives considered**: `tantivy` for full-text (rejected: it is a complete search engine with its own directory format and segment lifecycle — two storage engines in one product, and FR-037/FR-038 require layouts composed from *our* L0 primitives); depending on `hnsw_rs` at runtime (rejected: same reason, plus its locking model conflicts with our async access pattern); `simsimd` SIMD kernels (rejected: C bindings, Principle I).

## R21. Per-type metrics

**Decision**: track per `(type, namespace, container)`: `accesses_total{op_kind}`, `hits_total` / `misses_total` (types with a hit notion: caches, bloom filters, indexes, hybrid memory portions), `operation_duration_seconds` histogram, `bytes_stored`, `bytes_memory`, `items`. Names are reserved in `node::stats` following `08`'s conventions with labels `type`, `level`, `namespace`, `container`, `mode`, `codec`; exposition stays `08`'s. Container-level series are aggregatable to type level so a namespace with thousands of containers does not force per-container scraping.

**Rationale**: FR-044 requires the counts and the labels for `08` series and `07` quotas; the constitution's Observability Contract forbids renaming `08`'s labels, so this feature only adds.

**Alternatives considered**: per-container series only (rejected: cardinality explosion and `08`'s datatype-level series would have to be computed by the scraper); type-level only (rejected: `07` quotas are per namespace and per data type).

## R22. Conformance harness generation

**Decision**: the harness enumerates the **catalog**, not a hard-coded list: for each registered type it reads the descriptor and generates the matrix of modes × applicable encodings × codecs × encryption scopes, invokes every operation the descriptor claims, runs the starter example verbatim, round-trips the canonical representation, and asserts each capability-matrix entry. A descriptor claim that the implementation cannot honour fails CI (FR-049). A newly registered type therefore acquires its tests by existing.

**Rationale**: FR-049 makes an overstated catalog entry a defect; the only way to keep that true for 55 types and future additions is to derive the tests from the claims. It also gives FR-046 a cheap check — registering a test type must not change any other test's outcome.

**Alternatives considered**: hand-written tests per type (rejected: 55 types × 3 modes × 5 encodings × 4 codecs × 2 scopes is not hand-writable, and new types would ship untested); property-based fuzzing only (rejected: it does not check that a *claim* matches behaviour, which is the actual requirement).

---

## Resolved Technical Context summary

| Item | Resolution |
|---|---|
| Language/Version | Rust 1.87, edition 2024, MSRV 1.85 (inherited from `001`) |
| Pure-Rust codecs | `snap`, `structured-zstd` (+`ruzstd` cross-check), `lz4_flex` — R10 |
| Pure-Rust crypto | RustCrypto `aes-gcm`, `chacha20poly1305`, `hkdf`, `zeroize` — R12 |
| Pure-Rust structures | `roaring`, `kiddo`, `rstar`, `unicode-segmentation` — R20 |
| ANN strategy | in-house HNSW and ScaNN with crate oracles in dev-dependencies — R20 |
| Storage | node-local `data_dir`: catalog log + snapshots, per-container WAL/SSTable/segments — R7, R8, R9 |
| Testing | catalog-driven conformance generation — R22 |
| Interface compatibility | additive-only growth of `002`'s traits, CI-verified — R2 |
| Interim seams | `KeyAuthority` keyring file (`07`), `LocalDirector` placement (`04`), local `CatalogStore` (`06`) — R7, R12, R16 |
