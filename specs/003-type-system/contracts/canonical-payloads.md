# Contract: Canonical Representation — Payloads for Every Type

**Feature**: `003-type-system` | Crate: `crates/types` (`canonical.rs`) | Extends [`002`'s canonical representation](../../002-protocol-drivers/contracts/canonical-representation.md) | Spec: FR-042

The envelope, media type and canonicalisation rules of `002` are **unchanged**: `{"$d":<payload>,"$type":"<type>/<kind>","$v":1}`, `application/vnd.spacestorage.canonical+json; v=1`, RFC 8785-style sorted keys, `{"$b":…}` bytes, `{"$t":…}` timestamps, `{"$n":…}` big integers, `{"$f":…}` non-finite floats, byte-stable round-trip. Only the payload table grows, and per-type versioning replaces the implicit global one.

## 1. Per-type versioning

Each descriptor declares `canonical { current_version, accepted_versions, payload_kinds }`. A type emits `current_version` and accepts every listed version (FR-042). A payload change is therefore a bump on one type, not a global `$v` change — `002`'s drivers keep carrying bytes unaltered either way.

## 2. Payload kinds

`value`, `row`, `doc`, `item`, `entry`, `element`, `segment`, `member`, `object-meta`, `composition-meta`, `op-args`, `op-result`. `$type` is `<type_name>/<kind>`.

## 3. Payload schemas

### L0 data structures (`element` unless noted)

| `$type` | `$d` |
|---|---|
| `tuple/element` | `{"fields": [any, …]}` (arity fixed by schema) |
| `vector/element` | `{"i": int, "v": any}` |
| `linked_list/element` | `{"v": any, "prev": string?, "next": string?}` |
| `deque/element` | `{"v": any, "end": "front" \| "back"}` |
| `ring_buffer/element` | `{"seq": int, "v": any}` |
| `hash_table/entry` | `{"k": any, "v": any}` |
| `bplus_tree/entry` | `{"k": any, "v": any}` |
| `skip_list/entry` | `{"k": any, "v": any}` |
| `radix_tree/entry` | `{"k": string, "v": any}` |
| `heap/element` | `{"priority": number \| string, "v": any}` |
| `bloom_filter/op-args` (`maybe_contains`) | `{"item": any}` → `op-result` `{"maybe": bool, "fp_rate": float}` |
| `kd_tree/item` | `{"id": string, "point": [float…], "payload": object?}` |
| `hnsw/item`, `scann/item` | `{"id": string, "vector": [float…], "payload": object?}` |
| `bitmap/op-args` (`rank`/`select`) | `{"pos": int}` → `op-result` `{"value": int}` |

Shared ANN op payloads (`hnsw`, `scann`, `kd_tree`, `vector_collection`, `vector_search`):
`op-args` (`knn`) `{"vector": [float…], "k": int, "filter": Expr-JSON?, "ef": int?}`;
`op-result` `[{"id": string, "score": float, "payload": object?}]` — identical to `002`'s `vector_collection` shapes, so drivers need no change.

### L2 abstractions

| `$type` | `$d` |
|---|---|
| `map/entry`, `ordered_map/value` | `{"v": any}` (`ordered_map/value` kept verbatim from `002`) |
| `multimap/entry` | `{"k": any, "values": [any…]}` |
| `set/element`, `ordered_set/element` | `{"v": any}` |
| `sequence/element` | `{"idx": int, "v": any}` |
| `document/doc` | `{"_id": string, …arbitrary JSON…}` |
| `field_index/entry` | `{"key": any, "refs": [string…]}` |
| `field_path/entry` | `{"path": string, "v": any}` |
| `kv_collection/value` | `{"v": string \| {"$b":…} \| {"h": {field: string}}, "ttl_ms": int?}` (verbatim from `002`) |
| `bitmap_index/entry` | `{"key": any, "bitmap": {"$b":…}}` |
| `ngram_index/entry` | `{"gram": string, "postings": [{"id": string, "pos": [int…]}]}` |
| `range_index/entry` | `{"key": any, "refs": [string…]}` |
| `spatial_index/item` | `{"id": string, "geo": {"$geo": …}, "payload": object?}` |
| `vector_collection/item` | `{"id": string, "vector": [float…], "payload": object?}` (verbatim from `002`) |
| `timeseries_segment/segment` | `{"start": {"$t":…}, "end": {"$t":…}, "points": [{"ts": {"$t":…}, "values": {field: number}}]}` |
| `object/object-meta`, `object_collection/object-meta` | `{"content_type": string, "content_length": int, "etag": string, "last_modified": {"$t":…}, "user_meta": {k: v}}` (verbatim from `002`; body is a separate byte stream) |

### L3 models

| `$type` | `$d` |
|---|---|
| `relational_table/row` | `{"<column>": scalar, …}` (verbatim from `002`) |
| `columnar_table/row` | same as `relational_table/row`; batch form `{"columns": {name: [values…]}}` under `op-args` |
| `document_store/doc` | `{"_id": string, …arbitrary JSON…}` (verbatim from `002`) |
| `fulltext_search/doc` | `{"_id": string, "fields": {name: string}}`; `op-args` (`search`) `{"q": string, "mode": "match"\|"phrase"\|"fuzzy", "limit": int}` → `op-result` `[{"id": string, "score": float, "highlights": {field: [string…]}?}]` |
| `vector_search/item` | as ANN above |
| `spatial_search/item` | `{"id": string, "geo": {"$geo": …}, "payload": object?}`; `op-args` (`within`/`intersects`/`nearest`) `{"geo": {"$geo": …}, "k": int?, "radius_m": float?}` |
| `kv_store/value` | `{"v": any, "ttl_ms": int?}` |
| `timeseries/item` | `{"ts": {"$t":…}, "key": object, "values": {field: number}}`; `op-args` (`aggregate_window`) `{"from": {"$t":…}, "to": {"$t":…}, "window": string, "fn": "avg"\|"sum"\|"min"\|"max"\|"count"}` |
| `object_storage/object-meta` | as `object_collection/object-meta` |
| `log_stream/entry` | `{"offset": int, "ts": {"$t":…}, "v": any}` |

### L4 compositions

| `$type` | `$d` |
|---|---|
| `<kind>/member` | `{"id": string, "name": string, "type": string, "state": "ready"\|"missing"}` |
| `<kind>/composition-meta` | `{"kind": string, "members": [member…], "rule": object, "depth": int, "write": "read_only"\|"routed", "refresh": {"policy": string, "last": {"$t":…}?, "staleness_ms": int?}?}` |

Reads through a composition return the **member's** payload kind unchanged, so a client reading a `union` of two `relational_table`s sees `relational_table/row` envelopes.

## 4. New scalar wrapper

`{"$geo": {"type": "Point"\|"Polygon"\|…, "coordinates": …}}` — GeoJSON geometry, canonicalised by the same key-sorting rules. This is the only wrapper `003` adds to `002`'s set.

## 5. Test obligations (SC-008)

For every registered type and every payload kind it declares: build a value, emit canonical bytes, parse them back, emit again, assert byte identity; assert that a value of the wrong shape fails with `CanonicalError::SchemaMismatch{path, expected}` and stores nothing; assert that every `accepted_versions` entry parses. `002`'s existing vectors are re-run unchanged as a regression gate on the six inherited names.
