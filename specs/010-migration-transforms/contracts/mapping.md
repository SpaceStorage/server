# Contract: Mapping

**Feature**: `010-migration-transforms` | Crates: `migrate` + catalog `003` + exec `005` | Spec: FR-014

## Catalog mapping (understandable)

On a type descriptor (or pair):

```json
{
  "from": "document_store",
  "to": "relational_table",
  "fields": [{ "src": "id", "dst": "id" }, { "src": "email", "dst": "email" }]
}
```

Same-type `re_encode` / `re_compress` / `re_encrypt` / `sharding_key` (key is an existing field) imply identity mapping. No `MappingQuery` required.

Conflicting types for one dest field → job `failed` naming documents/fields; source unchanged.

## Mapping query (complex)

Required when there is no catalog default, data is schema-free/nested without default, there is more than one source container, or the column layout is not the default.

```json
{
  "sources": [
    { "namespace": "acme", "container": "events", "columns": ["id", "payload.ts"] }
  ],
  "destinations": [
    { "namespace": "acme", "container": "events_sql", "columns": ["id", "ts"] }
  ],
  "filter": null
}
```

Lowering: `LogicalRequest` scan + project + insert (`005`). This crate does **not** parse SQL.

Slice 10: no join, no aggregate. Those plans → `NotSupported { what: mapping_join }`.

Missing source container/column → `MappingSourceMissing`. Complex job without query → `MappingQueryRequired`.

An explicit query on an understandable pair **overrides** the catalog mapping.
