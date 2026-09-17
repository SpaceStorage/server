# Data Model: Data Migration and Type/Model Transforms

**Feature**: `010-migration-transforms` | **Date**: 2026-09-18

Entities for [spec.md](spec.md) and [plan.md](plan.md). Persistence: [R3](research.md). Validation: [contracts/](contracts/).

## 1. Job

Cluster- or namespace-scoped record. Identity: `JobId` (UUID v7).

| Field | Type | Notes |
|-------|------|--------|
| `id` | UUID | Immutable |
| `kind` | `JobKind` | See §2 |
| `status` | `JobStatus` | State machine §3 |
| `paused` | bool | True when dual-write/target apply failed; `08` status stays `running` |
| `principal_id` | UUID | Submitter (`014`) |
| `created_at` / `updated_at` | HLC | `012` |
| `progress` | `JobProgress` | §4 |
| `error` | optional named error | Quota, name, mapping, constraint, key, state |
| `strategy` | `Strategy` | `live` (default), `snapshot`, `offline` |

### 1.1 Validation

- Unknown kind → refuse.
- First-binary profile → `MigrateSlice10Required`.
- Authz per [R15](research.md).

## 2. JobKind

| Variant | `08` `job` label | Body |
|---------|------------------|------|
| `DataMigration` | `data_migration` | `MigrationSpec` |
| `DataTransformation` | `data_transformation` | `TransformSpec` |
| `DataBackup` | `data_backup` | `BackupSpec` |
| `DataRestore` | `data_restore` | `RestoreSpec` |

Do not emit `backup` / `snapshot` for these jobs; those tokens remain `13` mechanics.

## 3. JobStatus

```text
starting → running → completed
                 ↘ failed
cancel requested from running/starting → failed (source intact; target incomplete)
```

`paused` is a flag on `running`, not a fifth `08` status.

Resume: from `running` (including paused) using `progress.last_applied_source_seq`. If recorded state is unreadable → `failed` naming the state (FR-008). Never silently drop durable source acks.

## 4. JobProgress

| Field | Type |
|-------|------|
| `bytes_copied` | u64 |
| `objects_copied` | u64 |
| `bytes_remaining` | optional u64 (estimate) |
| `objects_remaining` | optional u64 |
| `install_seq` | optional source seq at interceptor install |
| `last_applied_source_seq` | optional |
| `last_source_seq` | optional (head) |

Queryable while `running` (FR-007, `09`).

## 5. MigrationSpec

| Field | Type | Notes |
|-------|------|--------|
| `source` | `ContainerRef` | namespace + name |
| `dest_namespace` | optional name | If set and ≠ source ns → copy or move |
| `dest_name` | optional name | Default = source name; must be free in dest ns |
| `policy` | `copy` \| `move` | Move drops source after cutover |
| `dest_nodes` / `dest_drives` | optional | Node/drive migrate; labels via `004` |
| `replica_slots` | optional list | Subset of replicas to move; default all that the dest policy requires |

### 5.1 Validation

- Dest name exists → `NameExists`.
- `04` unsatisfiable → refuse constraint.
- Quota start fail → refuse quota (`07`).
- Same ns + no dest nodes/drives + copy → refuse `NoOp`.

## 6. TransformSpec

| Field | Type | Notes |
|-------|------|--------|
| `source` | `ContainerRef` | |
| `rewrite` | `RewriteKind` | §7 |
| `target_type` | optional catalog type | Required for type/model conversion |
| `mapping` | `Catalog` \| `Query(MappingQuery)` | §8 |
| `new_name` | optional | Free name for the new container; default generated |
| `swap` | bool | Default true for in-namespace rewrite |
| `retain_source` | bool | Default **false** (drop previous after swap) |

## 7. RewriteKind

| Kind | Understandable without query? |
|------|-------------------------------|
| `TypeModel` | Only if catalog `default_transform` exists |
| `IncompatibleSchema` | Only if catalog mapping covers dropped/renamed/narrowed fields |
| `ReEncode` / `ReCompress` | Yes (identity map) |
| `ReEncrypt` | Yes (identity map; new key ref required) |
| `ShardingKey` | Yes if new key is a declared field; else mapping query |

In-place attempts stay refused by `03`/`04` with a pointer here.

## 8. MappingQuery

| Field | Type |
|-------|------|
| `sources` | `[{ namespace, container, columns[] }]` columns = field paths or `*` |
| `destinations` | `[{ namespace, container, columns[] }]` |
| `filter` | optional `005` predicate (comparisons only in slice 10) |

### 8.1 Validation

- Complex source without query → `MappingQueryRequired`.
- Understandable + extra query → query **overrides** catalog (allowed).
- Missing container/column → `MappingSourceMissing { name }`.
- Join/agg IR → `NotSupported` until slice 8.
- Lowers to `LogicalRequest` only ([contracts/mapping.md](contracts/mapping.md)).

## 9. CatalogMapping

Stored on the **target** type descriptor in `003` (or a pair table in the catalog):

```text
default_transform { from: DocumentStore, to: RelationalTable, fields: [{ src: "id", dst: "id" }, …] }
```

Identity mapping is implied for same-type codec/key/shard rewrites.

## 10. BackupSpec / RestoreSpec

| Backup | Restore |
|--------|---------|
| `scope`: container or namespace | `snapshot_id` |
| `pitr`: bool (record WAL position) | optional `pitr_position` |
| output: `snapshot_id`, WAL position from `13` | `dest` cluster/namespace |
| | optional `key_ref` (`14`) |

No snapshot blob in this crate.

## 11. IncompleteTarget

A container with `incomplete: true`, name `ss:job:<job_id>`. Relationships: `job_id` → Job. Not in tenant lists. Dropped on successful move source-drop / transform default swap, or left for operator drop on cancel.

## 12. DualWriteWindow

| Field | Type |
|-------|------|
| `container_id` | UUID |
| `job_id` | UUID |
| `install_seq` | seq |
| `target_id` | UUID |

At most one live window per source container. Second job on the same source → refuse `JobInProgress`.

## 13. Relationships

```text
Job 1──1 MigrationSpec | TransformSpec | BackupSpec | RestoreSpec
TransformSpec 0..1 MappingQuery
TransformSpec 0..1 CatalogMapping (looked up, not copied)
Job 1──0..1 IncompleteTarget
Job 1──0..1 DualWriteWindow
BackupSpec / RestoreSpec ──invokes── 13 Snapshot
```

## 14. State transitions (cutover/swap)

Preconditions (all MUST hold):

1. `last_applied_source_seq` ≥ last source seq
2. Quota re-check passes
3. Placement re-check passes (`04`)
4. Target not `incomplete` after a final `complete` mark
5. Dest public name still free **or** is the swap source name

Then: publish name (swap or copy dest), drop source if move, drop previous if transform swap && !retain_source, unregister interceptor, `status=completed`.
