# Contract: Cluster registry (namespace records)

**Feature**: `007-tenancy-security` | Crate: `tenancy` | Spec: FR-001, FR-006, FR-013, FR-014 | Seam: `006` cluster log

## Cluster log MUST contain

- `NamespaceRecord` (id, name, quotas, policies, private flags, group_id)
- Role and RoleBinding rows (`roles.md`)

MUST NOT contain schemas or container definitions (`006` FR-003).

## Ops

| Op | Who | First binary |
|----|-----|----------------|
| `NamespaceCreate { name }` | `CLUSTER_ADMIN` | yes; starts namespace Raft (`06` R5) |
| `NamespaceRename { id, new_name }` | `CLUSTER_ADMIN` | yes; unique index only; id/`group_id` unchanged |
| `NamespaceDelete { id, cascade }` | `CLUSTER_ADMIN` | yes; `cascade=false` + containers → `CascadeRequired` |
| List / get | any principal allowed by `14` to see that tenant; `CLUSTER_ADMIN` sees all | yes; **cluster applied state only** (SC-009) |

`NAMESPACE_ADMIN` create/delete/cascade/rename → `NotClusterAdmin`.

Rename: new binds use `new_name`; in-flight sessions stay on **id** until disconnect. Usage ledger stays keyed by id.

## Commit rule

Succeeds only with a majority of **cluster** voters. Minority → `Minority { group: cluster }`.

## Bootstrap

`cluster { starter_namespace acme; }` (or documented starter) creates `acme` if missing. No implicit tenant if the directive is omitted.
