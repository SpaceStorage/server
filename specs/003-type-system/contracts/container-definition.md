# Contract: Container Definition and Validation

**Feature**: `003-type-system` | Crate: `crates/types` (`definition.rs`, `validate.rs`) | Spec: FR-015–FR-020, FR-029

A container is created from one **definition**. The definition is a flat, documented option namespace so every protocol can carry it through `002`'s type option on its native create verb, and so `spacestorage` CLI, admin API and SQL/CQL `WITH` clauses all say the same thing.

## 1. Flat option namespace

| Option | Value | Default | Requirement |
|---|---|---|---|
| `type` | type name from the catalog | — (mandatory) | FR-015 |
| `mode` | `memory \| persistent \| hybrid` | descriptor default | FR-021 |
| `hybrid.window` / `hybrid.fraction` | duration / 0.0–1.0 | per `HybridPolicy` | FR-022 |
| `schema` | canonical JSON schema document (see [schema.md](schema.md)) | — | FR-017a |
| `layout.<role>` | L0 primitive name (`primary`, `index`, `filter`, `log`, `cache`, `dictionary`, `graph`) | descriptor `default_layout` | FR-037, FR-038 |
| `layout.<role>.<param>` | scalar | primitive default | |
| `encoding.<field \| data_kind>` | encoding name | `plain` or type default | FR-025 |
| `compression` | `none \| snappy \| zstd \| lz4` | descriptor `default_codec` | FR-026 |
| `compression.level` | integer in the codec's range | codec default | |
| `encryption.algorithm` | `aes-256-gcm \| chacha20-poly1305` | `aes-256-gcm` | FR-027 |
| `encryption.key` | key reference (e.g. `kv-rambler/data/ss/tenant-a`) | — | FR-027 |
| `encryption.scope` | `drives \| drives_and_memory` | `drives` | FR-027a, Clarification Q5 |
| `capability.<name>.<param>` | see [shared-capabilities.md](shared-capabilities.md) | none declared | FR-031, FR-032 |
| `compose.kind` / `compose.members` / `compose.rule.*` / `compose.refresh.*` | see [composition.md](composition.md) | — | FR-007, FR-008a |
| `if_not_exists` | bool | `false` | |

Unknown option ⇒ `InvalidCreateOption{name, reason}` listing the options this type accepts. A definition with **no** options beyond `type` must succeed for every creatable type (FR-029, SC-002).

### Example (CLI / admin form)

```text
spacestorage create tenant-a.metrics \
  type=timeseries \
  mode=hybrid hybrid.window=7d \
  schema=@schema-metrics.json \
  layout.primary=timeseries_segment layout.log=wal \
  encoding.ts=delta encoding.value=gorilla \
  compression=zstd compression.level=3 \
  encryption.algorithm=aes-256-gcm encryption.key=kv-rambler/data/ss/tenant-a encryption.scope=drives \
  capability.replication.factor=3 capability.replication.anti_affinity=az \
  capability.partitioning.scheme=time capability.partitioning.key=ts capability.partitioning.interval=1d
```

### Protocol projections (owned by `002`, shown for continuity)

| Protocol | Carrier of the same options |
|---|---|
| PostgreSQL / ClickHouse SQL | `CREATE TABLE … WITH (type='timeseries', mode='hybrid', …)` |
| Cassandra CQL | `CREATE TABLE … WITH spacestorage = {'type': 'timeseries', …}` |
| Redis | `SS.CREATE <name> TYPE timeseries MODE hybrid …` |
| Elasticsearch | `PUT /{index}` body `{"spacestorage": {"type": "...", …}}` |
| S3 | `PUT /{bucket}?spacestorage-op=create` with the option document |
| WebDAV | `MKCOL` with the `spacestorage:definition` property |

## 2. Validation pipeline (whole-definition, allocation-free on failure)

Order is normative; every stage collects all errors of its stage before failing, and **no storage, no placement and no catalog record is created unless every stage passes** (FR-018, SC-005).

1. **Type resolution** — name exists, `creatable`, not deprecated → `UnknownType`, `NotCreatable`, `TypeDeprecated`.
2. **Identity** — namespace exists or is implicitly created; name unused in it → `AlreadyExists`.
3. **Schema** — required elements present, domains valid, key definable for the type → `SchemaRequired`, `TypeMismatch`.
4. **Mode** — supported by the type *and* by every layout component; hybrid params within policy → `UnsupportedStorageMode`.
5. **Layout** — every component ∈ `composed_from`; roles satisfied; params valid → `LayoutComponentNotAllowed`.
6. **Encodings** — each applies to its data kind, and the field exists in the schema → `EncodingNotApplicable`.
7. **Compression** — codec registered, level in range → `UnknownCodec`.
8. **Encryption** — algorithm registered; scope legal for the mode; key reference resolvable now → `UnknownAlgorithm`, `EncryptionScopeInvalid`, `KeyUnresolvable`.
9. **Capabilities** — each supported by the type; parameters declarable; no mutual conflict → `CapabilityUnsupported`, `CapabilityConflict`.
10. **Composition** (L4 only) — members exist, same namespace, acyclic, depth ≤ 8, kind-specific rule valid, union-compatibility → `CompositionCycle`, `CompositionCrossNamespace`, `CompositionDepthExceeded`.
11. **Commit** — catalog record written and fsynced (`CatalogStore`), then storage allocated, then placement declared to `PlacementDirector`.

## 3. Alter

| Change | Result |
|---|---|
| `type` | refused: `TypeError::TypeImmutable` with a transform hint (FR-015, Story 1 scenario 8) |
| additive schema change | applied in place, `schema.version += 1` (FR-017b) |
| incompatible schema change | `IncompatibleSchemaChange{change, transform_hint}` |
| `compression`, `compression.level` | applies forward; old data readable; both codecs listed (FR-028) |
| `encoding.<field>` | applies forward; history recorded |
| `encryption.key` | new version prepended; old kept for existing blocks (FR-028) |
| `encryption.scope` | widening takes effect as memory data repopulates; narrowing immediate |
| `encryption.algorithm` | applies forward under a new key version |
| `capability.<name>.<param>` | validated; refused if the matrix marks it fixed (FR-035) |
| `layout.*`, `mode` | refused: transform (data movement) — `10` |
| `compose.members` | allowed for `union`/`federated`; validated for cycles and namespace |
| rename | allowed; compositions unaffected (members are ids) |

## 4. Drop

- No dependants → container removed, name reusable, storage released, catalog record appended.
- Dependants and no cascade → `ContainerHasDependants{dependants}` (FR-019).
- With cascade → dependent compositions are dropped, or updated when their kind tolerates a missing member, and the change is recorded in their description (FR-019, [composition.md](composition.md)).

## 5. Describe

`describe` returns exactly the fields FR-017 lists — name, namespace, id, type, level, kind, schema, mode, layout components, encodings per data kind, codecs present, encryption algorithm/key-reference/scope, declared capabilities or "placement defaults", composition members and dependants, state, stats — with a `defaults` marker per option. **Key material never appears** (SC-011).

## 6. Validation codes → protocol errors

Every code above maps through `002`'s `ErrorRenderer` to the connecting protocol's error form, carrying the offending element and the accepted alternatives. Fixtures for each code live in [fixtures/invalid/](fixtures/invalid/) and the conformance suite asserts one case per code (SC-005).
