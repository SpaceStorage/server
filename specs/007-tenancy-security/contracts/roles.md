# Contract: Roles and bindings

**Feature**: `007-tenancy-security` | Crate: `tenancy` | Spec: FR-005, FR-007, FR-013 | Verbs: `014`

## Builtin roles (first binary)

| Name | Scope | Meaning |
|------|--------|---------|
| `admin` | cluster | May act across namespaces. Gate for registry/quota writes is `CLUSTER_ADMIN` (`14`). First-binary admin token (`001`) maps here. |
| `replication` | cluster (nodes) | Authenticates on `internode` / `replication` only. `ReplicationNotTenant` on tenant protocols. |

Stored in cluster-level controller storage. Survive restart (SC-008).

## Custom roles (slice 7)

Named sets of `14` verbs. Namespace-scoped. Empty permission set: may authenticate if bound; can do nothing else. First binary: `RolePut` of a custom role → `Slice7Required`.

## Bindings

- Non-admin principal: **exactly one** `namespace_id`. Unbound → refuse on tenant protocols (`02`/`14`).
- `admin`: cluster scope; MAY omit namespace.
- `replication`: not a tenant binding.

This crate stores bindings; it does not implement SCRAM, tokens, or mTLS (`14`). Bindings use **principal id**, so a login rename in `14` MUST NOT rewrite this table.
