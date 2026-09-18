# Contract: Permission vocabulary

**Feature**: `014-authz-keys` | Crate: `authz` | Spec: FR-003, FR-004 | Research R9, R13, R16 | Roles stored in `007`

## Closed verbs

`CLUSTER_ADMIN` `NAMESPACE_ADMIN` `READ` `WRITE` `CREATE` `DROP` `CONFIGURE` `REPLICATE` `MIGRATE` `AUDIT_READ` `METRICS_READ`

Expanding the list is a constitution-adjacent amendment, not a silent code add.

## Implication

A principal that has `CLUSTER_ADMIN` (builtin `admin`, or a slice-7 custom role that includes the bit) is allowed **every** other verb on **every** namespace. Checks MUST NOT require extra `READ`/`WRITE` grants.

## Builtins (first binary)

| Role | Verbs | Notes |
|------|-------|--------|
| `admin` | `{CLUSTER_ADMIN}` | Cluster scope. Immutable. |
| `replication` | `{REPLICATE}` | Node identity on `internode`/`replication` only. `ReplicationNotTenant` on tenant handlers. Immutable. |

`RolePut` / `RoleDelete` / rename of builtins → `BuiltinRoleImmutable`. Custom name `admin` or `replication` → `RoleNameReserved`.

## First-binary tenant default

Until slice 7 custom roles exist, a namespace-bound principal that is **not** `admin` or `replication` receives an implicit grant `{READ, WRITE, CREATE, DROP, CONFIGURE}` **on that namespace only**. This is not a stored custom role. It exists so PG/Redis smoke can use a bound login (unbound `admin` is refused on Redis). Slice 7 **replaces** the implicit grant with explicit custom roles (empty custom set = authenticate only).

## Slice 7 custom roles

Named sets of the closed verbs, stored by `007`. Namespace-scoped. `READ`/`WRITE`/`CONFIGURE` MAY list `container_ids` (empty = all containers in the namespace). `CREATE`/`DROP` are namespace-wide. First-binary `RolePut` of a custom role → `Slice7Required`. A custom set that includes `CLUSTER_ADMIN` gets implication. UIs (`09`) MUST call this `Authorizer`; no private privilege model.

## Authorizer

```text
authorize(principal_id, verb, resource{namespace_id?, container_id?}) -> Allow | Deny
```

Deny is the protocol's authorization error (`002`). `CONFIGURE` does not grant namespace-registry or quota writes (`007` FR-014).
