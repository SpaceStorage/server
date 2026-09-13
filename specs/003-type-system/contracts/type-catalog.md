# Contract: Type Catalog, Descriptors and Registration

**Feature**: `003-type-system` | Crate: `crates/types` (`descriptor.rs`, `catalog.rs`, `lib.rs`) | Spec: FR-009–FR-014, FR-045–FR-049

The catalog is the single source of type knowledge for protocol drivers (`002`), the query planner (`05`), the admin surfaces (`001`) and the conformance harness. Nothing else may hold a type list.

## 1. Descriptor schema

Every registered type carries this record; **all fields are mandatory** (FR-045).

```rust
pub struct TypeDescriptor {
    pub name: TypeName,                     // snake_case, unique across all levels
    pub display_name: &'static str,         // spec spelling, e.g. "B+tree"
    pub level: Level,                       // L0 | L1 | L2 | L3 | L4 — exactly one
    pub kind: Kind,                         // DataStructure | StoragePrimitive | StorageLayout | SharedCapability | Abstraction | StorageModel | Composition
    pub storage_modes: StorageModes,        // { supported, default, hybrid: Option<HybridPolicy> }
    pub schema_kind: SchemaKind,            // Required | Optional | Free
    pub schema_required_elements: &'static [SchemaElement],
    pub operations: &'static [OperationSpec],
    pub capabilities: CapabilityMatrixRow,  // one entry per registered shared capability
    pub composed_from: &'static [TypeName],
    pub default_layout: Layout,
    pub encodings: &'static [(DataKind, &'static [EncodingName])],
    pub codecs: &'static [CodecName],
    pub default_codec: CodecName,
    pub encryption: EncryptionSupport,      // { supported, default_scope }
    pub canonical: CanonicalInfo,           // { current_version, accepted_versions, payload_kinds }
    pub deprecation: Option<Deprecation>,   // { since, replacement, note }
    pub availability: Availability,         // Stable | Preview
    pub starter_example: &'static str,      // a container definition that runs verbatim
}
```

`creatable` is **derived** from `kind` (`DataStructure | Abstraction | StorageModel | Composition`), never declared, so a descriptor cannot contradict Clarification Q1.

### Validation at registration

| Code | Rule |
|---|---|
| `IncompleteDescriptor{missing}` | every field present; builder cannot finish otherwise |
| `MultipleLevels` | exactly one level named |
| `DuplicateType{name}` | name unused across all levels |
| `LayoutNotInComposedFrom{primitive}` | `default_layout` ⊆ `composed_from` |
| `DefaultCodecNotListed` | `default_codec` ∈ `codecs` |
| `DefaultModeUnsupported` | `storage_modes.default` ∈ `storage_modes.supported` |
| `HybridPolicyMissing` | `Hybrid` ∈ supported ⇒ `hybrid` is `Some` |
| `StarterExampleInvalid{reason}` | the example parses and validates against this descriptor |
| `UnknownCapabilityInMatrix{name}` | matrix rows reference registered capabilities only |

## 2. Registration

```rust
TypeSystem::builder()
    .register_encoding(EncodingDescriptor { … })?
    .register_codec(CodecDescriptor { … })?
    .register_algorithm(AlgorithmDescriptor { … })?
    .register_capability(CapabilityDescriptor { … })?
    .register_type(Arc<dyn Datatype>)?          // descriptor comes from Datatype::descriptor()
    .build()                                    // -> Arc<TypeSystem>, immutable
```

Rules (FR-002, FR-045–FR-048):

1. One registration point: `crates/typeset`. No link-time collection, no dynamic loading.
2. Registering anything leaves every existing descriptor and every existing container untouched (FR-046, SC-010).
3. A newly registered **capability** is `Unsupported` for every existing type until that type's descriptor says otherwise (FR-031).
4. A newly registered **encoding / codec / algorithm** becomes selectable for new containers immediately; existing containers keep theirs (FR-046).
5. `deprecation: Some(_)` ⇒ create refuses with `TypeDeprecated{type, replacement}`; existing containers stay fully usable (FR-047).
6. Removing a type is refused while any container of it exists anywhere in the cluster: `TypeInUse{type, containers}` (FR-048).

## 3. Catalog read surface

Through the abstract interface (drivers, planner):

```rust
sys.types()                       -> impl Iterator<Item = &TypeDescriptor>   // stable order: level, then name
sys.type_(name)                   -> Option<&TypeDescriptor>
sys.creatable_types()             -> impl Iterator<Item = &TypeDescriptor>   // 002's startup mapping check
sys.encodings() / codecs() / algorithms() / capabilities()
sys.operations(type_name)         -> &[OperationSpec]                        // metadata only, no container access
sys.supports(type_name, op)       -> bool
sys.release()                     -> ReleaseId
```

Through the admin API (`001` ops, token-protected) and the CLI:

| Admin op | HTTP | CLI | Returns |
|---|---|---|---|
| `types` | `GET /v1/types?level=L3` | `spacestorage types [--level L3]` | name, display, level, kind, creatable, modes, availability, deprecation |
| `type` | `GET /v1/types/{name}` | `spacestorage type relational_table` | the full descriptor incl. operations, capability row, starter example |
| `codecs` | `GET /v1/codecs` | `spacestorage codecs` | encodings, codecs, algorithms with applicability |
| `capabilities` | `GET /v1/capabilities` | `spacestorage capabilities` | capability descriptors + which types support each |
| `catalog` | `GET /v1/catalog` | `spacestorage catalog [--cluster]` | release id + digest; `--cluster` reports per-node differences (FR-011) |
| `containers` | `GET /v1/namespaces/{ns}/containers` | `spacestorage containers <ns>` | id, name, type, level, mode, state |
| `describe` | `GET /v1/namespaces/{ns}/containers/{name}` | `spacestorage describe <ns>.<name>` | the full container description (FR-017) |

Catalog reads never touch container data (FR-039) and are safe on a `draining` node.

## 4. Catalog identity and per-node differences

`ReleaseId` = build version + a digest over all descriptors in stable order. Two nodes on the same release produce the same digest (FR-011). `catalog --cluster` lists nodes whose digest differs and diffs the type name sets, so an operator sees an asymmetric rollout. A container of a type a node does not know is restored as `Unavailable{UnknownType}` on that node and placed only on nodes that know it (spec edge case).

## 5. Documentation generation

`docs/types/<name>.md` is generated from the descriptor at release: display name, level, kind, modes, schema kind, operation table, capability row, layout options, codecs, canonical payload kinds, and the starter example. Every page whose type supports `memory` carries the same generated sentence stating that memory mode is a **volatile tier** and that durability requires persistent mode, hybrid mode or replication; the `types` and `type` outputs carry it too, so the statement FR-023 requires is in the catalog and not only in prose. FR-013 (documentation consistent with the catalog) is therefore structural — a documented item that is absent from the catalog cannot be generated, and a catalog item that is undocumented fails the generation step in CI.

## 6. Conformance obligations (FR-049)

The harness reads this catalog and, for every descriptor:

- runs the starter example verbatim on a fresh node (SC-003),
- creates containers across every supported mode × applicable encoding × codec × encryption scope (SC-004),
- invokes every `OperationSpec` through the abstract interface and checks `ordering`, `filtering` and `mutates` claims (SC-007),
- asserts every `CapabilitySupport` row by declaring the capability and expecting acceptance or `CapabilityUnsupported` (SC-006),
- round-trips the canonical payload kinds (SC-008).

A claim the implementation cannot honour fails CI. A `Preview` type must therefore under-claim rather than over-claim.
