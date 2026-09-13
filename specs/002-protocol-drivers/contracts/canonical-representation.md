# Contract: Canonical Representation (v1)

**Feature**: `002-protocol-drivers` | Module: `crates/types/src/canonical.rs` | Spec: FR-014, FR-015, FR-015a; Clarification Q4

One canonical, self-describing, versioned representation per type. Every driver uses it as the **fallback** for types without a native protocol representation and **accepts it on write for every type**. Drivers carry it **unaltered** inside their protocol's natural carrier.

## Media type

`application/vnd.spacestorage.canonical+json; v=1`

## Envelope

```json
{"$d":<payload>,"$type":"<type>/<kind>","$v":1}
```

| Key | Meaning |
|-----|---------|
| `$type` | `<type_name>/<kind>` where `kind ∈ value \| row \| doc \| object-meta \| item \| op-args \| op-result`, e.g. `vector_collection/item`, `relational_table/row`, `kv_collection/value` |
| `$v` | representation version (integer); v1 is this document; drivers accept every documented version, emit the current |
| `$d` | payload shaped by `Datatype::canonical_schema()` |

Keys appear in sorted order (`$d`, `$type`, `$v`) because of the canonicalisation rules below.

## Canonicalisation rules (normative)

1. UTF-8, no BOM, no whitespace outside strings.
2. Object keys sorted by UTF-16 code unit sequence (as RFC 8785 §3.2.3); duplicate keys are invalid.
3. Integers: JSON number without fraction/exponent, exact for `i64`/`u64` (values outside → string with `$n` wrapper: `{"$n":"123456789012345678901234567890"}`).
4. Floats: shortest round-trip decimal (`ryu`), lowercase `e`, no `+`, `-0` preserved as `-0.0`; `NaN`, `Infinity`, `-Infinity` as strings under `{"$f":"NaN"}`.
5. Bytes: `{"$b":"<base64url, no padding>"}`.
6. Timestamps: `{"$t":"2026-09-13T17:00:00.123456Z"}` (RFC 3339, UTC, microseconds, trailing zeros trimmed to at least seconds).
7. Strings: minimal escaping (`"`, `\`, control chars as `\uXXXX`; `/` unescaped); non-ASCII emitted raw.
8. `null`, `true`, `false` literal.
9. Arrays keep order.

Property: `to_canonical_bytes(from_canonical_bytes(b)) == b` for any valid `b` (byte stability, Story 2 scenario 4a); equal logical values yield identical bytes.

## Carriers per protocol

| Protocol | Read carrier | Write carrier(s) accepted |
|----------|--------------|---------------------------|
| postgresql | column type `jsonb` (also `text`/`bytea` when the query casts) | `jsonb`, `json`, `text`, `bytea` literal or parameter |
| cassandra | `text` | `text`, `blob` |
| redis | bulk string (raw UTF-8 bytes) | bulk string |
| elasticsearch | JSON value (the envelope object itself) in `_source` for foreign-type containers; raw body for `_spacestorage` ops | JSON body |
| clickhouse / clickhouse-http | `String` column (also `JSON` when client selects) | `String`, `FORMAT JSONEachRow` field |
| s3 | object body with the media type as `Content-Type` | object body |
| webdav | entity body with the media type | entity body |

The canonical bytes are byte-identical across all carriers except where the carrier itself is a JSON document (Elasticsearch `_source`), in which case the envelope object is embedded verbatim and re-canonicalises to the same bytes.

## Interim type payload schemas (`$d`)

| `$type` | `$d` |
|---------|------|
| `kv_collection/value` | `{"v": string \| {"$b":…} \| {"h": {field: string}}, "ttl_ms": int?}` |
| `relational_table/row` | `{"<column>": scalar, …}` (columns per container `columns` option) |
| `document_store/doc` | `{"_id": string, …arbitrary JSON…}` |
| `object_collection/object-meta` | `{"content_type": string, "content_length": int, "etag": string, "last_modified": {"$t":…}, "user_meta": {k: v}}` (body is separate byte stream) |
| `vector_collection/item` | `{"id": string, "vector": [float…] (len = dims), "payload": object?}` |
| `vector_collection/op-args` (`knn`) | `{"vector": [float…], "k": int, "filter": Expr-JSON?}` |
| `vector_collection/op-result` (`knn`) | `[{"id": string, "score": float, "payload": object?}]` |
| `ordered_map/value` | `{"v": any}` |

Feature `03` extends this table; new schemas get their own `$type` and reuse `$v` rules.

## Validation errors

`CanonicalError { position: usize, reason: NotUtf8 | Syntax | DuplicateKey | UnsortedKeys | UnknownType | UnsupportedVersion | SchemaMismatch{path, expected} }` → rendered per protocol as `InvalidOptionValue`/`TypeMismatch` class errors (never stored partially).

## Test vectors

`crates/types/tests/canonical_vectors/*.json` — pairs of (logical value, expected bytes) including: nested sorted keys, `-0.0`, `1e21`, `NaN`, u64 max, i64 min, bytes empty/one/three, timestamp with and without micros, non-ASCII strings, empty object/array.
