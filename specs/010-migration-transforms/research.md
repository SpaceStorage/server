# Research: Data Migration and Type/Model Transforms

**Feature**: `010-migration-transforms` | **Date**: 2026-09-18

Each item resolves a Technical Context unknown or a technology choice. Format: Decision / Rationale / Alternatives considered. Inputs: [spec.md](spec.md) (clarify 2026-09-18), constitution 1.3.0, intent `10`, sibling plans `003` (schema/type refuse → transform), `004` (rebalance vs migrate), `005` (one IR), `006` (cluster/namespace logs), `007` (logical quota), `008` (job series names), `013` (snapshot/PITR), `014` (`MIGRATE`), `016` (slice 10).

## R1. One crate `migrate`, not a jobs sidecar

- **Decision**: Add `crates/migrate` (`spacestorage-migrate`). It owns job records, dual-write, transform, mapping, and backup/restore **orchestration**. `013` still owns compaction/flush/checkpoint/vacuum/GC/TTL/snapshot **mechanics**. `004` still owns replica repair.
- **Rationale**: Constitution III. Spec: this feature owns migration/transform behavior and backup **jobs** that invoke `13`.
- **Alternatives considered**: Separate `jobs` crate used by `13` (rejected for slice 10: `13` already runs compaction without this); sidecar migrator (rejected: monolith).

## R2. First binary vs slice 10

- **Decision**: First binary (`016` slices 1–5): `JobService` is a stub. Create/list of `data_*` jobs → `MigrateSlice10Required`. Config `jobs { enabled on; }` on a first-binary profile is a startup error. **Slice 10** implements this plan together with `013` snapshot/PITR APIs.
- **Rationale**: `016` slices table. Operators following first-binary quickstart must not need evacuate-by-job.
- **Alternatives considered**: Ship live migrate in first binary (rejected: depends on `006` job persist and `013` snapshot strategy); silent no-op (rejected: named error).

## R3. Job persistence: cluster vs namespace log

- **Decision**: Persist `JobRecord` via `006` `ClusterStore.append`. Route like other metadata: **cluster log** when the job names two namespaces, node/drive placement, or backup scope wider than one namespace; **namespace log** when source and dest are the same namespace. Resume: any `ready` node reads applied job state and continues catch-up from `last_applied_source_seq`.
- **Rationale**: Constitution XII; jobs must survive the worker node. `006` already routes cluster vs namespace events.
- **Alternatives considered**: Local files only (rejected: lose job on evacuate of that node); always cluster log (rejected: tenant transform would pollute cluster Raft).

## R4. Dual-write interceptor on the write path

- **Decision**: Register a `WriteInterceptor` on the source container. During the window, a counted source acknowledgement waits until (1) source durable/memory ack as today and (2) the **mapped** write is applied on the target at the target's write quorum. Catch-up scans keys/rows with `source_seq <= install_seq` and upserts idempotently. Cutover/swap only when `last_applied_source_seq` ≥ last source seq. If target apply fails: **do not ack** the client write; set job `paused` with the named error; operator resumes or cancels (source intact).
- **Rationale**: Clarify Q2; spec edge case. Freeze was rejected. Snapshot-at-start would drop live acks from the target.
- **Alternatives considered**: Async queue to target (can ack then lose); two-phase commit per write (too heavy; interceptor + target quorum is enough).

## R5. Source sequence is the container's source-log / HLC seq (`12`)

- **Decision**: `source_seq` is the per-container sequence already used for replica catch-up (`012` source log / HLC in the source `quorum_domain`). Job progress stores `last_applied_source_seq` and `install_seq`. Idempotent apply: same logical key + seq is a no-op if already present.
- **Rationale**: Do not invent a second CDC stream. `04` repair already consumes that log; this feature MUST NOT be the only repair path (FR-012) but MAY read the same log for catch-up.
- **Alternatives considered**: Byte offsets in SSTables (rejected: not logical); wall clock (rejected: skew).

## R6. Strategies

- **Decision**: `live` (default node-to-node and default transform): interceptor + catch-up + cutover. `snapshot`: job invokes `13` snapshot of source, restore into dest (disaster / offline maintenance). `offline`: refuse new writes on source (`paused_writes`) until copy completes — documented, not default.
- **Rationale**: Spec Key Entities. Live matches clarify. Snapshot reuses `13` (FR-006).
- **Alternatives considered**: Only snapshot (rejected: 20-minute evacuate of a live KV would stall writers); only live (rejected: disaster restore needs snapshot strategy).

## R7. Placement validation is `04`, execution is this crate

- **Decision**: Before start and before cutover, call `PlacementDirector` with the **destination** replica set (nodes/drives/labels). Unsatisfiable anti-affinity or RF → refuse naming the constraint. After cutover, `04` owns the new replica set; this job must not leave extra replicas that violate policy (SC-001). Node-to-node migrate of "some replicas" means the job names **which replica slots** move; remaining slots stay. Changing RF is `04`/`03` capability change, not this job.
- **Rationale**: Spec US1; FR-009; FR-012.
- **Alternatives considered**: Migrate crate places independently (rejected: second placement engine).

## R8. Namespace move drops the source

- **Decision**: After successful cutover, drop the source container (`003` drop): name reusable, storage released. Copy leaves source. No empty shell. Incomplete dest is dropped or marked incomplete on failure; never swapped in.
- **Rationale**: Clarify Q4; SC-002.
- **Alternatives considered**: Empty container (rejected: name/quota leak); tombstone-as-row-delete (wrong layer).

## R9. Transform swap default: drop previous

- **Decision**: Optional swap is atomic rename: public name points at the new container. **Default** `retain_source=false`: drop the previous container after swap (same as move). `retain_source=true` keeps it under a generated name ` <old>__pre_transform_<job_id>` only if that name is free; else refuse swap naming the collision.
- **Rationale**: Spec left retain-or-drop "per policy". Tests and quotas need a default. Mirrors move.
- **Alternatives considered**: Always retain (quota double until manual drop).

## R10. Destination name collision

- **Decision**: Copy/move/transform **new-container** name that exists → refuse `NameExists { container }` before copy. Transform swap of the **source's own name** is the atomic rename, not a collision. Restore-in-place name reuse is `13` after drop or explicit restore-replace.
- **Rationale**: Clarify Q3; `003` uniqueness.
- **Alternatives considered**: Overwrite/merge (rejected).

## R11. Quota: start + cutover, no reservation

- **Decision**: At start, ask `07` whether destination namespace logical usage + source logical size (second copy) fits. If not, refuse before create. Before cutover/swap, re-read usage; if over, fail job named, source intact, target `incomplete` and not listed to tenants. Do **not** lease quota. Dual-write consuming writes that exceed dest quota fail as ordinary `07` rejects (and pause the job if the interceptor cannot apply).
- **Rationale**: Clarify Q5 option B; `007` best-effort hard reject, no serialize through cluster primary.
- **Alternatives considered**: Reservation protocol (Q5 D, extra cluster object); cutover-only (hidden partial copy).

## R12. Incomplete targets are not tenant-visible

- **Decision**: New containers created by jobs have `incomplete: true` and an internal name `ss:job:<uuid>` until cutover/swap assigns the public name. Tenant `LIST` / protocol names omit incomplete containers. `CLUSTER_ADMIN` and the job principal may describe them via job status. Cancel: source intact; incomplete target stays marked (operator may drop); never swapped. A later `cleanup` job (`08` name already reserved, behavior may be this crate) MAY drop stale incomplete targets; not required for slice 10 SC.
- **Rationale**: Spec: no partial tenant-visible target.
- **Alternatives considered**: Public temp names (tenants would see half-copied tables).

## R13. Catalog mapping vs mapping query

- **Decision**: Type catalog (`003`) MAY declare `default_transform: { from, to, fields: [{src, dst}] }` on a type pair. **Understandable** = that declaration exists, **or** rewrite kind is same-type re-encode / re-encrypt / sharding-key (identity field map). **Complex** = otherwise, including schema-free/nested without default, >1 source container, or non-default columns. Complex **requires** a `MappingQuery` on the job:
  - `sources[]`: `{ namespace, container, columns[] }` where column is a field path or `*`
  - `destinations[]`: `{ namespace, container, columns[] }`
  - optional `filter` as a `005` predicate already representable on first-binary CRUD (comparisons on columns)
  - No join/aggregate in slice 10; those IR variants → `NotSupported` until slice 8
  Lower to `LogicalRequest` (scan + project + insert). This crate MUST NOT parse SQL itself; admin/CLI may send the AST JSON. Optional later: handler SQL `INSERT … SELECT` lowers in `002`/`005` to the same AST.
- **Rationale**: Clarify extra mapping rule; constitution IV; `005` one IR.
- **Alternatives considered**: Free SQL string in the job (second parser); always require a query (rejected: understandable types).

## R14. Backup/restore jobs only orchestrate `13`

- **Decision**: `DataBackup` stores scope (container or namespace), optional PITR flag, and the `snapshot_id` + WAL position returned by `13`. `DataRestore` names snapshot, optional PITR position, dest cluster/namespace, optional key reference. Encrypted: pass through `14` key refs; missing key → fail named (SC-005). No parallel snapshot file format.
- **Rationale**: FR-006; `013` US4.
- **Alternatives considered**: tar of data_dir (rejected: not crash-consistent at WAL position).

## R15. Authz

- **Decision**: Node/drive migrate: `MIGRATE` on the container **or** `CLUSTER_ADMIN`. Same-namespace transform: `MIGRATE` (or `CONFIGURE`+`MIGRATE` if we must distinguish — **use `MIGRATE` only**). Cross-namespace copy/move/restore: `CLUSTER_ADMIN` **or** the same principal holds `MIGRATE` on **both** namespace ids. Dual grant is an AND of two namespace bindings (`014`), not a new verb.
- **Rationale**: Spec FR-010 "admin or documented dual MIGRATE grants".
- **Alternatives considered**: Any `NAMESPACE_ADMIN` can cross-ns (rejected: isolation).

## R16. Metrics and progress

- **Decision**: Increment existing `08` families with `job` ∈ {`data_migration`,`data_transformation`,`data_backup`,`data_restore`} and `status` ∈ {`starting`,`running`,`completed`,`failed`} (plus `paused` as `running` with job object `paused=true` so we do **not** add a fourth required status token). Progress on the job object: `bytes_copied`, `objects_copied`, `bytes_remaining` (estimate), `objects_remaining`, `last_applied_source_seq`. `09` map reads this object (FR-007).
- **Rationale**: Observability contract: do not rename series; `paused` is job-object state.
- **Alternatives considered**: New `spacestorage_migrate_*` families (unnecessary); add `paused` to the frozen `08` status enum (would be a catalog change).
