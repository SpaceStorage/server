# Contract: Usage and quota-reject metrics this crate emits

**Feature**: `007-tenancy-security` | Spec: FR-009 | Catalog: `008`

Figures only; exposition is `08`. Do not rename.

| Intent name | Prometheus name (reserved) | Type | Labels |
|-------------|----------------------------|------|--------|
| logical bytes | `spacestorage_namespace_usage_bytes` | gauge | `namespace` (current name; series identity SHOULD also expose `namespace_id`), optional `datatype` |
| logical objects | `spacestorage_namespace_usage_objects` | gauge | `namespace`, optional `datatype` |
| sessions | `spacestorage_namespace_connections` | gauge | `namespace` |
| quota rejects | `spacestorage_quota_exceeded_total` | counter | `namespace`, `unit` |

Physical replica disk series, if any, are `08` operator metrics and MUST NOT be these names. After namespace rename, `namespace` label follows the new name; `namespace_id` does not change.
