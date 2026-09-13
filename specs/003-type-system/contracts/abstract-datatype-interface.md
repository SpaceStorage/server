# Contract: Abstract Datatype Interface (v2 — owner)

**Feature**: `003-type-system` (owner) | Supersedes the provider role of [`002`'s contract](../../002-protocol-drivers/contracts/abstract-datatype-interface.md) | Crate: `crates/types` | Spec: FR-011, FR-012, FR-039–FR-041

`002` declared this surface and shipped an interim provider. `003` implements it for real. **Growth is additive only**: every method `002` relies on keeps its name, signature and semantics; everything new arrives as a default-bodied method, a new registry entry or a new `TypeError` variant. A CI job compiles `002`'s eight handler crates against every commit of `crates/types`.

## 1. Unchanged from `002` (drivers depend on these)

`TypeRegistry::{get, iter, register}`; `Datatype::{name, level, create_options, validate_create_options, open, operations, canonical_schema}`; `Container::{meta, get, put, delete, exists, scan, aggregate, type_op, object, alter, stats}`; `ObjectOps` in full; `Key`, `PutCond`, `PutOutcome`, `ScanQuery`, `AggQuery`, `Row`, `Expr`; the `TypeError` variants `002` renders.

Only the registry *contents* change: 6 interim types become 50 creatable ones. A driver that reads the registry rather than a hard-coded list gains them without an edit — which is exactly what `002` FR-013/FR-018 require.

## 2. Additive growth (all default-bodied)

```rust
pub trait Datatype: Send + Sync {
    // ... 002 methods ...

    fn descriptor(&self) -> &TypeDescriptor;                                   // FR-009 — the only non-defaulted addition
    fn kind(&self) -> Kind { self.descriptor().kind }
    fn creatable(&self) -> bool { self.descriptor().creatable() }              // FR-014
    fn schema_kind(&self) -> SchemaKind { self.descriptor().schema_kind }
    fn validate_definition(&self, def: &ContainerDefinition) -> Result<(), TypeError> { default_validation(self, def) }   // FR-016, FR-018
    fn validate_schema_change(&self, from: &ContainerSchema, to: &ContainerSchema) -> Result<(), TypeError> { additive_only(from, to) }  // FR-017b
    fn capability_support(&self, c: CapabilityName) -> CapabilitySupport { self.descriptor().capabilities.get(c) }        // FR-031
    fn supports(&self, op: &str) -> bool { self.operations().iter().any(|o| o.name == op) }                               // FR-039
    fn operation(&self, op: &str) -> Option<&OperationSpec> { … }
    fn canonical_payload(&self, kind: &str) -> Option<&CanonicalSchema> { … }                                            // FR-042
}

pub trait Container: Send + Sync {
    // ... 002 methods ...

    fn id(&self) -> ContainerId { self.meta().id }
    fn schema(&self) -> Option<&ContainerSchema> { None }                      // FR-017a
    fn layout(&self) -> &Layout { &self.meta().layout }
    fn mode(&self) -> Mode { self.meta().mode }
    fn state(&self) -> ContainerState { ContainerState::Ready }                // FR-023, FR-027
    fn describe(&self) -> ContainerDescription { default_describe(self) }      // FR-017
    async fn alter_schema(&self, change: SchemaChange) -> Result<u32, TypeError> { Err(NotSupportedByType…) }             // FR-017b
    async fn alter_capabilities(&self, delta: Vec<CapabilityDecl>) -> Result<(), TypeError> { … }                        // FR-035
    fn composition(&self) -> Option<&CompositionSpec> { None }                 // FR-007
    fn dependants(&self) -> &[ContainerId] { &[] }                             // FR-019
}
```

`ContainerMeta` gains `id`, `mode`, `layout`, `schema`, `encryption_scope`, `defaults_mask`, `state` — additive fields on a struct `002` only reads.

## 3. Rules for consumers

1. **One path.** Drivers (`002`) and the planner (`05`) use only this interface; no type-private access path exists (FR-040). Enforced by the crate graph: handler crates depend on `types`, `exec`, `auth`, `protocol-core` and nothing below (`storage`, `codec`, `crypto`, `l0`–`l4` are not in their dependency lists).
2. **No hidden types.** Listing a namespace returns every container with its type name, whatever the protocol (FR-013 of `002`, FR-020 here).
3. **Metadata-only capability queries.** `operations`, `supports`, `capability_support`, `descriptor` never open a container (FR-039).
4. **Refusal, not substitution.** An operation outside the set yields `TypeError::NotSupportedByType { type, op }` before any data access (FR-041).
5. **Canonical bytes are carried unaltered.** Drivers neither re-order nor re-encode them (`002` FR-015a); the representation is defined in [canonical-payloads.md](canonical-payloads.md).
6. **Startup coverage check.** `002` verifies at startup that each declared driver has a mapping for every `creatable_types()` entry; with 50 types this check now has teeth, and a missing mapping is a startup error naming driver and type.

## 4. Error additions

New `TypeError` variants (full list in [data-model.md §16](../data-model.md#16-errors-typeerror--fr-018)) all carry the offending element and the accepted alternatives, and are raised before any allocation. `002`'s `ErrorRenderer` maps unknown variants to its generic class for that protocol, so adding a variant never breaks a handler.

## 5. Versioning

The interface version is the crate's semver minor. Rules: adding a default-bodied method or a struct field ⇒ minor bump; changing a signature ⇒ forbidden while `002` is in tree (would require a coordinated feature). `Datatype::descriptor` is the single non-defaulted addition and lands in the same commit that converts the interim types, so nothing is ever half-migrated.
