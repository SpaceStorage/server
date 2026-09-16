# Contract: Access policies

**Feature**: `007-tenancy-security` | Crate: `tenancy` | Spec: FR-004 | Slice 7 (`tenancy-quotas`)

## Shape

`principal_or_role × datatype × allow verbs` inside one namespace. Datatype is a `03` type name or `*`.

## Who writes

Only `CLUSTER_ADMIN` (`PolicyReplace`). `NAMESPACE_ADMIN` → `NotClusterAdmin`. First binary → `Slice7Required`.

## Evaluation (data plane)

After role verbs (`14`), a matching policy MAY further deny a verb on a type (example: `WRITE` allowed on `K/V Store`, forbidden on `Log Stream`). Missing policy ⇒ role verbs apply to all types in the namespace.

UIs (`09`) MUST hit this same evaluation; no extra rights (FR-012).
