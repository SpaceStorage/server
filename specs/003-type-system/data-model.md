# Phase 1 Data Model: Multiparadigm Type System (L0–L4)

**Feature**: `003-type-system` | **Date**: 2026-09-13 | **Plan**: [plan.md](plan.md) | **Spec entities**: [spec.md § Key Entities](spec.md)

Records below are the in-memory and on-disk model. Rust shapes are indicative; the normative surface is the [contracts](contracts/). Every entity maps to one or more spec Key Entities and carries its governing requirement ids.

---

## 1. Levels and kinds (`crates/types/descriptor.rs`)

```rust
pub enum Level { L0, L1, L2, L3, L4 }                                   // FR-001: closed, exactly one per type

pub enum Kind {
    DataStructure,      // L0, creatable
    StoragePrimitive,   // L0, NOT creatable  (memtable, sstable, wal, append_segment)
    StorageLayout,      // L0, NOT creatable  (lsm_tree)
    SharedCapability,   // L1, never a container
    Abstraction,        // L2, creatable
    StorageModel,       // L3, creatable
    Composition,        // L4, creatable
}
```

**Invariants**

- `Level` is a closed enum; a new type picks one of the five (FR-001, FR-045). A parallel hierarchy is unrepresentable.
- `creatable == matches!(kind, DataStructure | Abstraction | StorageModel | Composition)` (FR-014, Clarification Q1). It is derived, not declared, so a descriptor cannot disagree with its kind.
- `Kind::SharedCapability` types have no `Container` implementation at all; they appear only in the capability registry and the per-type matrix (FR-030).

## 2. `TypeDescriptor` (spec: *Type*, *Type Descriptor*) — FR-009, FR-045

| Field | Type | Notes |
|---|---|---|
| `name` | `TypeName` (`snake_case`, unique across levels) | stable machine identity; `002`'s six names preserved |
| `display_name` | `&'static str` | spec spelling, e.g. `B+tree` |
| `level` | `Level` | exactly one (FR-045) |
| `kind` | `Kind` | drives `creatable` |
| `storage_modes` | `StorageModes { supported: EnumSet<Mode>, default: Mode, hybrid: Option<HybridPolicy> }` | FR-021, FR-022, FR-029 |
| `schema_kind` | `Required \| Optional \| Free` | FR-017a |
| `schema_required_elements` | `&[SchemaElement]` | e.g. `vector_search` requires a `vector(f32, dims)` field and a key |
| `operations` | `&[OperationSpec]` | FR-039; the whole operation set |
| `capabilities` | `CapabilityMatrixRow` | per shared capability: `Unsupported` or `Supported { declarable, fixed_after_create }` (FR-031) |
| `composed_from` | `&[TypeName]` | allowed layout components (FR-037, FR-038) |
| `default_layout` | `Layout` | FR-029; makes option-free creation work |
| `encodings` | `&[(DataKind, &[EncodingName])]` | applicability (FR-025) |
| `codecs` | `&[CodecName]` + `default_codec` | FR-026 |
| `encryption` | `EncryptionSupport { supported: bool, default_scope: Scope }` | FR-027a |
| `canonical` | `{ current_version: u16, accepted_versions: &[u16], payload_kinds: &[&str] }` | FR-042 |
| `deprecation` | `Option<Deprecation { since, replacement: Option<TypeName>, note }>` | FR-047 |
| `availability` | `Stable \| Preview` | preview types must under-claim (FR-049) |
| `starter_example` | `ContainerDefinitionText` | must run verbatim (FR-013, SC-003) |

**Invariants**: all fields mandatory at registration — a partially built descriptor cannot be finished (FR-045). `default_layout` ⊆ `composed_from`. `default_codec` ∈ `codecs`. Every `OperationSpec` must be exercised by the conformance harness (FR-049). For `Kind::StoragePrimitive | StorageLayout`, `operations` describe layout-internal behaviour and `starter_example` names the types that use the primitive instead of a create statement.

## 3. `OperationSpec` (spec: *Operation / Capability Descriptor*) — FR-039, FR-041

```rust
pub struct OperationSpec {
    pub name: &'static str,
    pub kind: OpKind,                    // Read | Write | Admin
    pub inputs: CanonicalSchema,
    pub outputs: CanonicalSchema,
    pub native: bool,                    // executed natively by this type
    pub ordering: Ordering,              // None | ByKey | ByField(&[&str]) | ByScore
    pub filtering: Filtering,            // None | Equality | Range | Prefix | Full
    pub cost_class: CostClass,           // O1 | LogN | Scan | Index
    pub mutates: bool,                   // kept from 002
    pub since_version: u16,
}
```

**Invariants**: answered from the registry with no container access (FR-039). An operation absent from the set produces `NotSupportedByType { type, op }` *before* any data access, never a substitute or partial result (FR-041).

## 4. `TypeCatalog` and `ContainerCatalog` (spec: *Type Catalog*) — FR-010–FR-013, FR-046–FR-048

```rust
pub struct TypeSystem {                          // immutable after build (R5)
    types: BTreeMap<TypeName, Arc<dyn Datatype>>,        // 002-compatible TypeRegistry view
    descriptors: BTreeMap<TypeName, TypeDescriptor>,
    encodings: BTreeMap<EncodingName, EncodingDescriptor>,
    codecs: BTreeMap<CodecName, CodecDescriptor>,
    algorithms: BTreeMap<AlgorithmName, AlgorithmDescriptor>,
    capabilities: BTreeMap<CapabilityName, CapabilityDescriptor>,
    release: ReleaseId,                                   // catalog identity for FR-011
}
```

`ContainerCatalog` is the mutable side, persisted by `CatalogStore` (§12):

| Field | Notes |
|---|---|
| `by_id: BTreeMap<ContainerId, ContainerRecord>` | authoritative |
| `by_name: BTreeMap<(NamespaceName, ContainerName), ContainerId>` | uniqueness (FR-015) |
| `dependants: BTreeMap<ContainerId, BTreeSet<ContainerId>>` | composition members → compositions (FR-019) |

**Invariants**: catalogs of two nodes on the same `release` are byte-identical when serialised (FR-011); the cluster view reports per-node differences. Registering an item leaves every existing descriptor and record untouched (FR-046, SC-010). A type with ≥ 1 live container cannot be removed (FR-048); it can be deprecated (FR-047).

## 5. `ContainerRecord` (spec: *Container*) — FR-015–FR-020

| Field | Type | Notes |
|---|---|---|
| `id` | `ContainerId` (UUID v7) | immutable identity (R6) |
| `namespace` | `NamespaceName` | exactly one (FR-015) |
| `name` | `ContainerName` | unique in namespace, renameable |
| `type_name` | `TypeName` | **immutable for life** (FR-015) |
| `schema` | `Option<ContainerSchema>` | present iff `schema_kind != Free` and declared (§6) |
| `mode` | `Mode` | memory \| persistent \| hybrid |
| `layout` | `Layout` | §8 |
| `encodings` | `BTreeMap<FieldOrKind, EncodingName>` | FR-025 |
| `codecs_present` | `Vec<CodecName>` | plural: old data keeps its codec (FR-028) |
| `encryption` | `Option<EncryptionSetting>` | §10, keys plural for the same reason |
| `capabilities` | `Vec<CapabilityDecl>` | §11 |
| `composition` | `Option<CompositionSpec>` | §13, present iff kind is `Composition` |
| `defaults_mask` | bitset | which options are defaults vs explicitly set (FR-017) |
| `state` | `ContainerState` | §14 |
| `created_at`, `altered_at` | timestamps | |
| `stats` | `ContainerStats` | §15 |

**Description projection** (`describe`) renders every field above plus level, kind, dependants, and for each option whether it is a default — and **never** key material (FR-017, SC-011).

## 6. `ContainerSchema` (spec: *Container Schema*) — FR-017a, FR-017b, Clarification Q4

```rust
pub struct ContainerSchema {
    pub fields: Vec<Field>,              // ordered; order is stable across evolution
    pub key: Option<KeyDef>,             // primary/partition/clustering per type
    pub paths: Vec<FieldPathDef>,        // declared field paths (document/JSON types)
    pub indexes: Vec<IndexDef>,          // declared secondary indexes over own fields
    pub params: BTreeMap<String, CanonicalValue>,   // type-specific (e.g. vector dims, metric)
    pub version: u32,                    // bumped by every accepted change
}
pub struct Field { name, domain: ValueDomain, nullable: bool, default: Option<CanonicalValue>, encoding: Option<EncodingName>, added_in: u32 }
```

`ValueDomain` — `bool | i8..i64 | u8..u64 | f32 | f64 | decimal(p,s) | string(max?) | bytes(max?) | timestamp | date | time | uuid | json | vector(f32, dims) | geo_point | geo_shape | enum{..} | array<T> | map<K,V> | struct{..}`.

**Invariants**: the schema is the single source of schema truth; drivers render it and keep none of their own (FR-017a). `schema_kind == Required` ⇒ creation without the required elements is refused. Field `added_in` lets a reader default fields absent from older blocks, which is what makes additive change free of rewriting (R15).

## 7. Schema evolution (state transitions) — FR-017b, SC-009a

```text
              additive change (add field | add default | widen domain | add index | add path)
  v_n  ────────────────────────────────────────────────────────────────────────►  v_{n+1}
   │                                                                                 │
   │ incompatible change (drop | rename | narrow | retype | key change | dim change) │
   ▼                                                                                 ▼
  REFUSED: TypeError::IncompatibleSchemaChange { change, transform_hint }      stored data unchanged;
                                                                               readers apply v_{n+1}
```

Widening lattice (normative table in [contracts/schema.md](contracts/schema.md)): `i8→i16→i32→i64`, `u8→u16→u32→u64`, `u_n→i_{>n}`, `f32→f64`, `decimal(p,s)→decimal(p',s)` for `p'>p`, `string(n)→string(m>n)→string`, `bytes` likewise, `enum` ∪ new values, `non-null → nullable`. Everything else is a transform (`10`).

Concurrency: a schema change is applied under the container's definition lock and is ordered against writes; a write validated under `v_n` stays valid under `v_{n+1}` because additive changes never invalidate stored data (spec edge case).

## 8. `Layout` and storage modes (spec: *Layout*, *Storage Mode*) — FR-021, FR-022, FR-037, FR-038

```rust
pub struct Layout { pub components: Vec<LayoutComponent> }
pub struct LayoutComponent { pub role: LayoutRole, pub primitive: TypeName, pub params: BTreeMap<String, CanonicalValue> }
pub enum LayoutRole { Primary, Index, Filter, Log, Cache, Dictionary, Graph }
pub enum Mode { Memory, Persistent, Hybrid }
pub enum HybridPolicy { Recent { window: Duration }, HotSet { fraction: f32 }, WriteBuffer, IndexResident }
```

**Invariants**: every `LayoutComponent.primitive` ∈ `descriptor.composed_from` (FR-038). Shared capabilities apply to the container as one unit — no component carries its own placement (FR-033), enforced by the absence of any per-component capability field. Mode must be in `storage_modes.supported` for the type **and** for every component primitive (FR-021). All three modes expose the same operation set (FR-022).

Mode semantics (Clarification Q2): `Memory` content is volatile; `Persistent` content is durable; `Hybrid` splits per `HybridPolicy` and reports which part is where.

## 9. Encoding and compression (spec: *Encoding*, *Compression Codec*) — FR-024–FR-026, FR-028

```rust
pub struct EncodingDescriptor { name, applies_to: &[DataKind], fallback: Option<EncodingName>, notes }
pub struct CodecDescriptor    { name, levels: RangeInclusive<u8>, default_level: u8 }
```

Pipeline order is fixed and not configurable: **encode → compress → encrypt** (FR-026, R9). A container may carry several codecs and encodings at once because a change applies forward only (FR-028); `codecs_present` and per-field `encoding_history` make that visible in the description.

Dictionary overflow: a block whose cardinality exceeds the type's dictionary policy falls back to `plain` for that block and records it — never a refused write (spec edge case).

## 10. `EncryptionSetting` (spec: *Encryption Setting*) — FR-027, FR-027a, Clarification Q5

```rust
pub struct EncryptionSetting {
    pub algorithm: AlgorithmName,        // aes-256-gcm (default) | chacha20-poly1305
    pub keys: Vec<KeyRefVersion>,        // current first; older kept so old blocks stay readable
    pub scope: Scope,                    // Drives (default) | DrivesAndMemory
}
```

**Invariants**: `mode == Memory && encryption.is_some()` ⇒ `scope == DrivesAndMemory`, else refused (FR-027a). An unresolvable `KeyRef` refuses creation, and at restart yields `ContainerState::Unavailable { reason: KeyUnresolvable }` with no plaintext access and no data loss (FR-027). Key material never appears in a record, description, log or config output (SC-011) — the type is `Zeroizing<[u8; 32]>` inside `crates/crypto` with no `Debug`/`Serialize`.

Scope widening (`Drives → DrivesAndMemory`) takes effect as memory-resident data is repopulated; the description shows requested vs fully-in-effect (spec edge case).

## 11. `CapabilityDecl` and the matrix (spec: *Shared Capability*, *Capability Compatibility Matrix*) — FR-030–FR-036

```rust
pub enum CapabilityDecl {
    Replication { factor: u16, anti_affinity: Option<LabelKey>, mode: ReplMode },
    Sharding { key: Vec<FieldPath>, shards: Option<u32> },
    Partitioning { scheme: PartScheme, key: Vec<FieldPath>, interval: Option<Duration> },
    NodePlacement { labels: LabelSelector },
    PersistentPlacement { labels: LabelSelector },
    Consistency { mode: ConsistencyMode },
    Quorum { write: QuorumLevel, read: QuorumLevel },
    FailureHandling, Rebalancing, DistributedTransactions, Consensus, LeaderElection,
}
pub enum CapabilitySupport { Unsupported, Supported { declarable: &'static [ParamName], fixed_after_create: &'static [ParamName] } }
```

**Invariants**: declaration form is identical at every level (FR-030). Validation against the matrix happens before anything reaches `PlacementDirector` (FR-032). Changing a `fixed_after_create` parameter is refused with a transform hint (FR-035). A capability registered later is `Unsupported` for every existing type until its descriptor says otherwise (FR-031, Story 4 scenario 7). A container with no declaration is described as using the director's defaults (FR-034).

## 12. `CatalogStore` and restore (spec: FR-023) — R7

```rust
pub enum CatalogRecord {
    CreateContainer(ContainerRecord), AlterOptions { id, delta }, SchemaChange { id, from: u32, to: u32, change },
    CapabilityChange { id, delta }, Rename { id, to }, Drop { id }, ComposeChange { id, delta },
}
```

On-disk: `catalog/catalog.log` (length-prefixed, CRC32, fsync before ack) + `catalog/catalog.snapshot.<n>`.

Restore state machine at boot:

```text
 load newest valid snapshot ──► replay log tail ──► for each container by mode:
        memory      → definition + options restored, content EMPTY, state = RestoredEmpty   (Clarification Q2)
        persistent  → WAL replay + SSTable open,      content intact, state = Ready         (FR-023)
        hybrid      → persistent part intact; memory part rebuilt per HybridPolicy, state = Ready
        key unresolvable → state = Unavailable{KeyUnresolvable}, no plaintext, nothing discarded
        type unknown on this node (older release) → state = Unavailable{UnknownType}, reported per node (FR-011)
```

## 13. `CompositionSpec` (spec: *Composition (L4)*) — FR-007, FR-008, FR-008a, R17

```rust
pub struct CompositionSpec {
    pub kind: CompositionKind,           // Union | Federated | MaterializedView | Distributed | Partitioned | Replicated | Sharded
    pub members: Vec<ContainerId>,       // by identity, never by name
    pub rule: CompositionRule,           // UnionOrder | FederatedRouting(pred) | ViewSource{sources, freshness} | Ranges | ShardKey | ReplicaTargets
    pub depth: u8,                       // computed; ≤ 8
    pub write: WriteRule,                // ReadOnly | RoutedSingleMember   (fixed per kind)
    pub tolerance: MissingMemberPolicy,  // ContinueAndRecord | RefuseAddressedRange | ServeStale
    pub refresh: Option<RefreshState>,   // materialized view: policy + last_refresh + staleness
}
```

**Invariants**: acyclic (walk at definition time); all members in the same namespace (FR-018); `write` is a property of the kind, not of the definition (Clarification Q3) — `Union` and `MaterializedView` are `ReadOnly`, the rest are `RoutedSingleMember` and refuse a write resolving to zero or several members with nothing stored. No composition provides multi-member atomicity; that is the L1 `DistributedTransactions` capability (FR-008a). Declaring capabilities on a composition governs the composition object; members keep their own (spec edge case).

## 14. `ContainerState` (lifecycle) — FR-016, FR-019, FR-023

```text
                 create (validated whole-definition, FR-018)
   (none) ──────────────────────────────────────────────────► Ready
      ▲                                                         │  │
      │ drop (no dependants, or cascade)  ◄─────────────────────┘  │ alter options / additive schema / capability change
      │                                                            ▼
   Dropped ◄── drop refused if dependants and no cascade ──────  Ready
                                                                  │
                          restart ─────────────────────────────►  RestoredEmpty (memory mode) | Ready (persistent/hybrid)
                          key unresolvable / unknown type ─────►  Unavailable { reason }
                          composition member lost ─────────────►  Degraded { missing: [ContainerId] }
```

## 15. `ContainerStats` and per-type metrics — FR-044, SC-012

`accesses_total{op_kind}`, `hits_total`, `misses_total`, `operation_duration_seconds` (histogram), `bytes_stored`, `bytes_memory`, `items`, labelled `type`, `level`, `namespace`, `container`, `mode`, `codec`. Aggregatable to type level; exposition belongs to `08`, quotas to `07`.

## 16. Errors (`TypeError`) — FR-018

`002`'s variants are preserved (`UnknownType`, `DuplicateType`, `InvalidCreateOption`, `MissingCreateOption`, `TypeMismatch`, `UnknownOperation`, `InvalidOperationArgs`, `NotFound`, `AlreadyExists`, `PreconditionFailed`, `Capacity`, `Internal`) and joined by:

`NotCreatable{type}`, `UnsupportedStorageMode{primitive, requested, supported}`, `EncodingNotApplicable{encoding, data_kind, applicable}`, `UnknownCodec`, `UnknownAlgorithm`, `EncryptionScopeInvalid{mode, scope}`, `KeyUnresolvable{key_ref}`, `SchemaRequired{elements}`, `IncompatibleSchemaChange{change, transform_hint}`, `CapabilityUnsupported{type, capability, supported}`, `CapabilityParamFixed{capability, param}`, `CapabilityConflict{a, b}`, `LayoutComponentNotAllowed{type, primitive}`, `CompositionCycle{path}`, `CompositionCrossNamespace{member}`, `CompositionDepthExceeded{limit}`, `CompositionReadOnly{kind}`, `CompositionWriteAmbiguous{resolved}`, `NotSupportedByType{type, op}`, `ContainerHasDependants{dependants}`, `TypeDeprecated{type, replacement}`, `TypeInUse{type, containers}`, `IncompleteDescriptor{missing}`, `MultipleLevels`.

Every variant names the offending element and the accepted alternatives, and is raised **before** any storage or placement allocation (FR-018, SC-005). Drivers map them through `002`'s `ErrorRenderer`.

---

## Relationships

```text
TypeSystem 1───* TypeDescriptor 1───* OperationSpec
                     │                      ▲
                     │ capabilities         │ operations answered from metadata (FR-039)
                     ▼                      │
         CapabilityMatrixRow          ContainerRecord ───1 ContainerSchema ───* Field
                     ▲                      │ │ │
CapabilityDecl ──────┘                      │ │ └──1 Layout ───* LayoutComponent ──► L0 TypeName
                                            │ └────1 EncryptionSetting ──► KeyAuthority (07 seam)
                                            └──────1 CompositionSpec ───* ContainerId (members)

ContainerCatalog ──persisted by──► CatalogStore ──replayed by──► restore (boot)
ContainerRecord ──executed through──► Datatype/Container (abstract interface) ──consumed by──► 002 drivers, 05 planner
CapabilityDecl ──handed to──► PlacementDirector (04 seam) ──bridges──► PlacementInfo (002 exec seam)
```
