# Contract: Container Schema and Evolution

**Feature**: `003-type-system` | Crate: `crates/types` (`schema.rs`) | Spec: FR-017a, FR-017b, FR-043 | Clarification Q4

The type system owns the schema. Protocol drivers render it in their own dialect and keep none of their own (FR-017a).

## 1. Schema document

```json
{
  "fields": [
    {"name": "id",    "domain": "uuid",                    "nullable": false},
    {"name": "ts",    "domain": "timestamp",               "nullable": false, "encoding": "delta"},
    {"name": "value", "domain": "f64",                     "nullable": false, "encoding": "gorilla"},
    {"name": "tags",  "domain": "map<string,string>",      "nullable": true},
    {"name": "embed", "domain": "vector(f32,768)",         "nullable": true}
  ],
  "key":     {"kind": "partition_clustering", "partition": ["id"], "clustering": [["ts", "asc"]]},
  "paths":   [{"name": "tags.host", "domain": "string"}],
  "indexes": [{"name": "by_host", "fields": ["tags.host"], "kind": "field_index"}],
  "params":  {"metric": "cosine", "retention": "30d"},
  "version": 1
}
```

## 2. Value domains

`bool`; `i8 i16 i32 i64`; `u8 u16 u32 u64`; `f32 f64`; `decimal(p,s)` (p ≤ 38); `string(max?)`; `bytes(max?)`; `timestamp` (µs, UTC); `date`; `time`; `uuid`; `json`; `vector(f32, dims)`; `geo_point`; `geo_shape`; `enum{a,b,…}`; `array<T>`; `map<K,V>` (K ∈ string, integer, uuid); `struct{name: T, …}`.

Each domain declares its canonical payload form ([canonical-payloads.md](canonical-payloads.md)) and its applicable encodings ([codecs.md](codecs.md)). A value that violates its domain, a key uniqueness rule, an ordering rule or a declared dimension is rejected naming the constraint, with nothing stored (FR-043).

## 3. Schema kinds per type

| Kind | Meaning | Types |
|---|---|---|
| **Required** | creation without the required elements is refused (`SchemaRequired{elements}`) | `relational_table`, `columnar_table`, `vector_search`, `spatial_search`, `timeseries`, `field_index`, `bitmap_index`, `range_index`, `ngram_index`, `spatial_index`, `vector_collection`, `timeseries_segment`, `kd_tree`, `hnsw`, `scann` |
| **Optional** | may be created bare; a schema may be added later and applies to subsequent writes | `document_store`, `fulltext_search`, `map`, `ordered_map`, `multimap`, `set`, `ordered_set`, `sequence`, `document`, `field_path`, `log_stream`, most L0 data structures |
| **Free** | no schema; values validated against the type's own constraints only | `kv_store`, `kv_collection`, `object_storage`, `object`, `object_collection`, `bloom_filter`, `bitmap` |

Adding a schema to a schema-free-in-practice container (Optional kind) is an additive change: it governs subsequent writes and never invalidates stored data (spec edge case).

## 4. Additive evolution — the widening lattice (FR-017b)

**Allowed in place, no data rewrite:**

| Change | Condition |
|---|---|
| add field | `nullable: true`, or a `default` is given |
| add path, add index | over existing fields |
| widen integer | `i8→i16→i32→i64`; `u8→u16→u32→u64`; `u_n → i_m` where `m > n` |
| widen float | `f32→f64` |
| widen decimal | `decimal(p,s) → decimal(p',s)` with `p' > p` (same scale) |
| widen string/bytes | `string(n) → string(m>n) → string`; same for `bytes` |
| extend enum | add values; never remove or reorder |
| relax nullability | `non-null → nullable` |
| extend struct | add a field to a `struct<…>` under these same rules |
| add array/map element widening | element domain widens under these rules |

**Refused — transform required (`10`):** drop field, rename field, narrow any domain, change a domain off the lattice, `nullable → non-null`, change key definition (partition or clustering), change vector `dims` or `metric`, change partitioning/sharding key, remove an index that a composition depends on.

Error: `IncompatibleSchemaChange { change, transform_hint }` naming the change and the transform that performs it.

## 5. Read-time application

A reader always applies the **current** schema to stored blocks:

- field absent from an older block → its `default`, else `null` (requires `nullable` or a default, which is why those are the additive preconditions);
- widened domain → value promoted on read (`i32` block value read as `i64`);
- `added_in` on each field records the schema version that introduced it, so a block written under `v_n` needs no rewrite to be read under `v_{n+m}`.

This is what makes an additive change complete in constant time regardless of container size.

## 6. Concurrency

A schema change takes the container's definition lock, is appended to the catalog log, then becomes visible. A write validated under `v_n` remains valid under `v_{n+1}` (additive changes never invalidate data), so writes are not blocked for the duration of the change — only the definition swap is serialised (spec edge case).

## 7. Cross-protocol visibility

Because there is one schema, a column added through PostgreSQL is visible in Cassandra's `system_schema`, in Elasticsearch's mapping response and in ClickHouse's `DESCRIBE TABLE` immediately — each driver renders the same document in its own dialect (spec edge case). Domain → protocol rendering tables live with each protocol contract in `002`; an unmappable protocol-native type is either mapped with stated precision or refused at definition time (`002` FR-017).
