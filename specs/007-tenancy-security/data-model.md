# Data Model: Namespaces, Quotas, Access Policies, Encryption Attachment, and Roles

**Feature**: `007-tenancy-security` | **Date**: 2026-09-16 | **Spec**: [spec.md](spec.md)

Control records apply from the **cluster** Raft log (`06`). Schemas/containers apply from **namespace** Raft. Usage ledgers are in-memory. Validation codes are in contracts.

## 1. NamespaceId / NamespaceName

| Field | Type |
|-------|------|
| id | UUID, assigned at create, immutable |
| name | unique cluster-wide, case-sensitive, `^[A-Za-z][A-Za-z0-9_]*$`, 1–63 chars; **renameable** |

On disk / log: id is the key; name is a unique secondary index on cluster state. `NamespaceRename` swaps the index; id and `group_id` do not change.

## 2. NamespaceRecord (cluster log)

| Field | Type |
|-------|------|
| id | NamespaceId |
| name | NamespaceName |
| quotas | list of QuotaSpec (empty = unlimited) |
| policies | list of AccessPolicy (empty until slice 7) |
| private_metrics | bool, default false |
| private_logs | bool, default false |
| group_id | `06` `GroupId::Namespace(id)` |
| created_hlc | Hlc |
| deleted | tombstone flag; cascade vs refuse is an op, not a state machine beyond this |

**Invariants**: list/get MUST NOT read namespace Raft. `private_* = true` is not enforceable until `08`. Rename changes `name` only.

## 3. QuotaSpec

| Field | Type |
|-------|------|
| unit | `bytes` \| `objects` \| `connections` \| `ops_per_sec` |
| limit | u64; `0` = reject consuming work immediately |
| datatype | optional `03` type name (per-type quota) |

**Invariants**: usage for `bytes`/`objects` is **logical** (one copy). RF MUST NOT multiply. `ops_per_sec` omitted from first binary. Unset unit = no check.

## 4. QuotaUsage (in-memory, not Raft)

| Field | Type |
|-------|------|
| namespace_id | UUID |
| unit | same as QuotaSpec |
| datatype | optional |
| current | u64 (best-effort) |
| as_of | Instant (local) |

Seeded from catalog logical sizes; adjusted by admit `fetch_add`/`fetch_sub` and internodes `QuotaDelta`; reconciled periodically.

## 5. Role

| Field | Type |
|-------|------|
| id | UUID |
| name | `admin` \| `replication` \| custom string (slice 7) |
| permissions | opaque list of `14` verbs; builtins documented |
| scope_kind | `cluster` (`admin`, `replication`) \| `namespace` (custom) |

**First binary**: only `admin` and `replication` rows exist.

## 6. RoleBinding

| Field | Type |
|-------|------|
| principal_id | UUID (`14` store; first binary: admin token, replication node id) |
| role_id | UUID |
| namespace_id | present iff non-admin; exactly one |

**Invariants**: non-admin without `namespace_id` → authentication refused on tenant protocols (`02`/`14`). `admin` MAY omit namespace. `replication` MUST NOT be used as a tenant.

## 7. AccessPolicy (slice 7)

| Field | Type |
|-------|------|
| namespace_id | UUID |
| principal_or_role | UUID |
| datatype | `03` type name or `*` |
| allow | set of `14` verbs (typically `READ`/`WRITE`/`CREATE`/`DROP`) |

Empty custom role ⇒ authenticate if bound, do nothing else. Missing policy + bound role verbs = those verbs on all types in the namespace.

## 8. EncryptionDeclaration (on container definition, namespace log)

| Field | Type |
|-------|------|
| algorithm | `AES-256-GCM` (default) \| `ChaCha20-Poly1305` |
| key_ref | string; never key bytes |
| scope | `drives` \| `drives_and_memory` |

Memory-mode + `drives` → refuse. Describe MUST show algorithm, key_ref, scope and MUST NOT show material.

## 9. Cluster tenancy ops (cluster log bodies)

```text
TenancyOp =
    NamespaceCreate { name }
  | NamespaceRename { id, new_name }
  | NamespaceDelete { id, cascade: bool }
  | QuotaReplace { id, quotas }          # slice 7; CLUSTER_ADMIN
  | PolicyReplace { id, policies }       # slice 7
  | RolePut { role }
  | RoleBindingPut { binding }
  | RoleBindingDelete { principal_id }
```

Commit rule: majority of **cluster** voters (`06`). Minority → `Minority { group: cluster }`.

## 10. NamespaceView (admin)

| Field | Type |
|-------|------|
| id, name | |
| quotas | specs + current usage (usage may lag) |
| role_bindings_count | u64 |
| containers | count from namespace catalog if reachable; `unknown` if namespace majority lost |
| encryption | not on this view (per container) |

Container list when namespace Raft is down: registry still lists the namespace; schema ops fail (`06`).

## Relationships

```text
ClusterState 1──* NamespaceRecord
NamespaceRecord 0..* QuotaSpec
NamespaceRecord 0..* AccessPolicy
ClusterState 1──* Role
Role 1──* RoleBinding
RoleBinding 0..1 NamespaceRecord
NamespaceRecord 1──1 namespace Raft group (schemas, containers, EncryptionDeclaration)
Coordinator 1──* QuotaUsage (memory)
```

## Validation codes

| Code | When |
|------|------|
| `NamespaceExists { name }` | duplicate name (create or rename target) |
| `NamespaceNameInvalid { name }` | charset/length |
| `NamespaceNotFound { name }` | old name after rename, or unknown |
| `CascadeRequired { name, containers }` | delete with containers and `cascade=false` |
| `NotClusterAdmin` | registry/quota/policy mutate without `CLUSTER_ADMIN` |
| `QuotaExceeded { quota, limit, usage }` | consuming request at/over cap |
| `QuotaUnitUnknown { unit }` | |
| `QuotaNegative` | limit overflow / invalid config |
| `Slice7Required { op }` | quota set / custom role / policy in first binary |
| `ObservabilityRequired { op }` | `private_metrics/logs on` before `08` |
| `KeyMaterialForbidden` | key bytes in declaration or describe |
| `EncryptionScopeInvalid { mode, scope }` | memory-mode + `drives` |
| `AccessDenied { verb, datatype, namespace }` | policy/role refuse (slice 7) |
| `ReplicationNotTenant` | replication role on a tenant protocol |
