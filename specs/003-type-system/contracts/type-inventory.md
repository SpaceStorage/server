# Contract: Type Inventory (55 types, 5 levels)

**Feature**: `003-type-system` | Crates: `l0`, `l2`, `l3`, `l4`, registered by `typeset` | Spec: FR-003–FR-008, FR-014

Machine names are `snake_case` and unique across levels. Modes: **bold** is the default. Schema: R = required, O = optional, F = free. Every name from the intent inventory appears exactly once; the list is expandable (FR-002).

Counts: **55 types** = 20 L0 (15 data structures + 4 storage primitives + 1 layout) + 18 L2 + 10 L3 + 7 L4, of which **50 are directly creatable**. L1 contributes 13 capabilities, which are not types (§L1 below).

---

## L0 — Foundation primitives

### Data structures (creatable — Clarification Q1)

| Name | Display | Modes | Schema | Default layout | Key operations |
|---|---|---|---|---|---|
| `tuple` | Tuple | **memory**, persistent | O | self | `get_field`, `set_field`, `arity`, `read`, `replace` |
| `vector` | Vector | **memory**, persistent, hybrid | O | self | `push`, `pop`, `get`, `set`, `len`, `truncate`, `range` |
| `linked_list` | Linked List | **memory**, persistent | O | self | `push_front`, `push_back`, `pop_front`, `pop_back`, `iterate`, `insert_after`, `remove` |
| `deque` | Stack / Queue / Deque | **memory**, persistent | O | self | `push_front`, `push_back`, `pop_front`, `pop_back`, `peek_front`, `peek_back`, `len` |
| `ring_buffer` | Ring Buffer | **memory**, hybrid | O | self | `append`, `read_from`, `capacity`, `oldest`, `newest`, `overwrite_count` |
| `hash_table` | Hash Table | **memory**, persistent, hybrid | O | self | `get`, `put`, `delete`, `exists`, `len`, `iterate` |
| `bplus_tree` | B+tree | memory, **persistent**, hybrid | O | self | `get`, `put`, `delete`, `range`, `prefix`, `first`, `last`, `iterate` |
| `skip_list` | Skip List | **memory**, hybrid | O | self | `get`, `put`, `delete`, `range`, `iterate` |
| `radix_tree` | Radix Tree / Patricia Trie | **memory**, persistent, hybrid | O | self | `get`, `put`, `delete`, `prefix`, `longest_prefix`, `iterate` |
| `heap` | Heap | **memory**, persistent | O | self | `push`, `pop_min`/`pop_max`, `peek`, `len`, `merge` |
| `bloom_filter` | Bloom Filter | **memory**, persistent, hybrid | F | self | `add`, `maybe_contains`, `false_positive_rate`, `union`, `clear` |
| `kd_tree` | KD-tree | **memory**, hybrid | R (`vector(f32, dims)`) | self | `insert`, `nearest`, `knn`, `range_search`, `delete` |
| `hnsw` | HNSW | memory, **hybrid**, persistent | R (`vector(f32, dims)`, `metric`) | self + `Graph` role | `insert`, `knn`, `delete`, `recall_estimate`, `rebuild` |
| `scann` | ScaNN | memory, **hybrid** | R (`vector(f32, dims)`, `metric`) | self + `Dictionary` role | `insert`, `knn`, `delete`, `retrain`, `rebuild` |
| `bitmap` | Bitmap / Bitset | **memory**, persistent, hybrid | F | self | `set`, `clear`, `test`, `and`, `or`, `xor`, `not`, `cardinality`, `rank`, `select` |

`hnsw` and `scann` accept `metric ∈ l2 | cosine | inner_product` and expose build parameters (`m`, `ef_construction`, `ef_search`; `leaves`, `leaves_to_search`, `reorder`) as schema `params`. `scann` ships `availability: Preview` until its recall oracle gate passes (research R20).

### Storage primitives (NOT creatable — Clarification Q1)

| Name | Display | Role | Used by |
|---|---|---|---|
| `memtable` | MemTable | `Primary` (in-memory write buffer) | `lsm_tree`, every LSM-backed L2/L3 type |
| `sstable` | SSTable | `Primary` (immutable sorted run) | `lsm_tree`, columnar and index models |
| `wal` | WAL | `Log` (durability, replay on boot) | every persistent and hybrid container |
| `append_segment` | Append-only Segment | `Log` / `Primary` | `log_stream`, `timeseries`, object part storage |

A create request naming one of these is refused with `NotCreatable{type}` and an error listing the types whose layouts use it (FR-014).

### Storage layouts / engines (NOT creatable)

| Name | Display | Components | Used by |
|---|---|---|---|
| `lsm_tree` | LSM Tree | `memtable` + `wal` + `sstable` (+ optional `bloom_filter`, `bplus_tree` index) | `kv_store`, `relational_table`, `document_store`, `map`-family, `timeseries` |

---

## L1 — Shared capabilities (13, not types)

`replication`, `sharding`, `partitioning`, `node_placement`, `failure_handling`, `rebalancing`, `consistency`, `persistent_placement`, `labels`, `distributed_transactions`, `consensus`, `leader_election`, `quorum`.

They are declared on containers of any level and validated against the per-type matrix; parameters, compatibility and the placement seam are in [shared-capabilities.md](shared-capabilities.md). They never appear as a container type (FR-030).

---

## L2 — Data abstractions (all creatable)

| Name | Display | Modes | Schema | Default layout | Key operations |
|---|---|---|---|---|---|
| `map` | Map | memory, **persistent**, hybrid | O | `lsm_tree` \| `hash_table` | `get`, `put`, `delete`, `exists`, `multi_get`, `scan` |
| `ordered_map` | Ordered Map | memory, **persistent**, hybrid | O | `lsm_tree` (+`bplus_tree` index) | `get`, `put`, `delete`, `range`, `prefix`, `first`, `last`, `scan` |
| `multimap` | Multimap | memory, **persistent**, hybrid | O | `lsm_tree` | `get_all`, `add`, `remove_value`, `remove_key`, `count`, `scan` |
| `set` | Set | memory, **persistent**, hybrid | O | `hash_table` \| `lsm_tree` | `add`, `remove`, `contains`, `cardinality`, `union`, `intersect`, `difference` |
| `ordered_set` | Ordered Set | memory, **persistent**, hybrid | O | `bplus_tree` \| `skip_list` | `add`, `remove`, `contains`, `range`, `rank`, `first`, `last` |
| `sequence` | Sequence | **persistent**, memory, hybrid | O | `append_segment` | `append`, `read_at`, `range`, `len`, `truncate_prefix` |
| `document` | Document abstraction | memory, **persistent**, hybrid | O | `lsm_tree` | `get`, `put`, `patch`, `delete`, `path_get`, `path_set`, `exists` |
| `field_index` | Field Index | memory, **persistent**, hybrid | R (indexed field) | `bplus_tree` \| `hash_table` | `lookup`, `range`, `insert`, `remove`, `cardinality` |
| `field_path` | Field Path | memory, **persistent**, hybrid | O | `radix_tree` | `get`, `put`, `delete`, `prefix`, `children`, `longest_prefix` |
| `kv_collection` | Key/Value collection | memory, **persistent**, hybrid | F | `lsm_tree` | `get`, `put`, `delete`, `exists`, `multi_get`, `multi_put`, `incr`, `expire`, `ttl`, `scan` |
| `bitmap_index` | Bitmap index | memory, **persistent**, hybrid | R (indexed field) | `bitmap` + `radix_tree` | `lookup`, `and`, `or`, `not`, `cardinality`, `insert`, `remove` |
| `ngram_index` | N-gram index | memory, **persistent**, hybrid | R (`n`, analyzer) | `radix_tree` + `sstable` postings | `index`, `search_substring`, `search_fuzzy`, `remove` |
| `range_index` | Range index | memory, **persistent**, hybrid | R (indexed field) | `bplus_tree` | `range`, `lookup`, `insert`, `remove`, `min`, `max` |
| `spatial_index` | Spatial Index Primitive | **memory**, hybrid, persistent | R (`geo_point`/`geo_shape`) | `kd_tree` \| R-tree | `insert`, `remove`, `within`, `intersects`, `nearest`, `bbox` |
| `vector_collection` | Vector collection | memory, **hybrid**, persistent | R (`vector(f32, dims)`) | `hnsw` (+`lsm_tree` payloads) | `put`, `get`, `delete`, `knn`, `filtered_knn`, `count` |
| `timeseries_segment` | Time-series segment | memory, **hybrid**, persistent | R (ts field + value fields) | `append_segment` + Gorilla/Delta | `append`, `range`, `downsample`, `last`, `compact` |
| `object` | Object | **persistent**, hybrid | F | `append_segment` | `put`, `get_range`, `head`, `delete`, `set_meta` |
| `object_collection` | Object Collection | **persistent**, hybrid | F | `append_segment` + `lsm_tree` index | `put`, `get_range`, `head`, `copy`, `delete`, `list_prefix`, `multipart_*` |

`kv_collection`, `ordered_map`, `vector_collection` and `object_collection` keep the machine names and operation names `002` already maps, so its per-protocol tables and canonical vectors survive unchanged.

---

## L3 — Storage models (all creatable)

| Name | Display | Modes | Schema | Default layout | Key operations |
|---|---|---|---|---|---|
| `relational_table` | Relational Table | memory, **persistent**, hybrid | **R** (columns, key) | `lsm_tree` + `field_index` per index | `insert`, `update`, `delete`, `scan`, `aggregate`, `join_source`, `point_get` |
| `columnar_table` | Columnar Table | memory, **persistent**, hybrid | **R** (columns) | `sstable` column chunks + `range_index` | `insert_batch`, `scan`, `aggregate`, `project`, `delete_where` |
| `document_store` | Document Store | memory, **persistent**, hybrid | O (mapping) | `lsm_tree` + `field_path` + `field_index` | `put`, `get`, `patch`, `delete`, `search`, `aggregate`, `path_query` |
| `fulltext_search` | Full-text Search | memory, **persistent**, hybrid | O (analyzers) | `ngram_index` + postings in `sstable` | `index`, `search`, `phrase`, `fuzzy`, `highlight`, `delete` |
| `vector_search` | Vector Search | memory, **hybrid**, persistent | **R** (`vector`, `metric`) | `vector_collection` over `hnsw`/`scann` | `index`, `knn`, `filtered_knn`, `rebuild`, `delete` |
| `spatial_search` | Spatial Search | **memory**, hybrid, persistent | **R** (geo field) | `spatial_index` + `lsm_tree` payloads | `index`, `within`, `intersects`, `nearest`, `bbox`, `delete` |
| `kv_store` | K/V Store | memory, **persistent**, hybrid | F | `lsm_tree` (+`bloom_filter`) | `get`, `put`, `delete`, `exists`, `multi_*`, `scan`, `incr`, `expire` |
| `timeseries` | Time Series | memory, **hybrid**, persistent | **R** (ts + values) | `timeseries_segment` rollups | `append`, `range`, `aggregate_window`, `downsample`, `retention_apply`, `last` |
| `object_storage` | Object Storage | **persistent**, hybrid | F | `object_collection` | `put`, `get_range`, `head`, `copy`, `delete`, `list_prefix`, `multipart_*` |
| `log_stream` | Log Stream | **persistent**, hybrid | O | `append_segment` + offset index | `append`, `read_from_offset`, `tail`, `truncate_prefix`, `offset_of_time` |

---

## L4 — Storage composition (all creatable)

| Name | Display | Write rule (Clarification Q3) | Members | Missing-member policy |
|---|---|---|---|---|
| `union` | Union | **read-only** | ≥ 2, union-compatible types | continue with remaining, record removal |
| `federated` | Federated | routed to the single member the routing predicate resolves | ≥ 2, heterogeneous allowed | continue with remaining, record removal |
| `materialized_view` | Materialized View | **read-only**, changes only by refresh | 1+ sources | serve last refreshed state, report staleness |
| `distributed` | Distributed | routed by placement rule | ≥ 1 | refuse operations addressing the missing member |
| `partitioned` | Partitioned | routed by partition range | ≥ 1 | refuse operations addressing the missing range |
| `replicated` | Replicated | routed to the replica set of the rule | ≥ 1 | refuse when no live target |
| `sharded` | Sharded | routed by shard key | ≥ 2 | refuse operations addressing the missing shard |

`distributed`, `partitioned`, `replicated` and `sharded` declare in their descriptors which L1 capabilities they compose (`node_placement`/`labels`, `partitioning`, `replication`, `sharding` respectively) and introduce none of their own (FR-008). Full semantics in [composition.md](composition.md).

---

## Union-compatibility table (FR-018, spec edge case)

`union` accepts members of the same type, plus these documented heterogeneous pairs: `relational_table` + `columnar_table` (identical column names and domains after widening), `document_store` + `document_store`, `timeseries` + `timeseries` (same ts field and value domains), `log_stream` + `log_stream`. Everything else is refused with a pointer to `federated`.

## Retirement of `002`'s interim inventory

| `002` interim type | Replaced by | Continuity |
|---|---|---|
| `kv_collection` | L2 `kv_collection` | same name, same ops, canonical `$type` unchanged |
| `relational_table` | L3 `relational_table` | same name; schema now owned by the type system (Clarification Q4) |
| `document_store` | L3 `document_store` | same name, canonical `$type` unchanged |
| `object_collection` | L2 `object_collection` | same name, `ObjectOps` unchanged |
| `vector_collection` | L2 `vector_collection` | same name, `knn` op args/result unchanged |
| `ordered_map` | L2 `ordered_map` | same name; `002`'s WebDAV lock container keeps working |

The in-memory-only implementations are deleted. This is the commit where data first survives a restart.
