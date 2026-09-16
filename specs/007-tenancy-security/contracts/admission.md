# Contract: Tenancy admission

**Feature**: `007-tenancy-security` | Crate: `tenancy` | Spec: FR-003 | Seams: `005` admission, `014` authz

## Order on a coordinator

1. Authenticate / bind namespace (`14` / `02`).
2. Role + access policy (`AccessDenied` if denied).
3. **Quota admit** (`QuotaExceeded` if `current >= limit` for a consuming unit).
4. Query admission (`005` node/namespace slots, memory). Stricter concurrent cap wins.

Unset quotas: skip step 3.

## Consuming vs non-consuming

| Work | Units |
|------|--------|
| Insert / put / append | `bytes`, `objects`, `ops_per_sec` |
| New authenticated session | `connections` |
| Read / describe / list | `ops_per_sec` only if that quota is set; not bytes/objects/connections |

On data-path failure after admit: decrement usage (best-effort).

## Errors

Named, never hang: `quota_exceeded { quota, limit, usage }`. Sibling namespaces unaffected.
