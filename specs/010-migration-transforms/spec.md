# Feature Specification: Data Migration and Type/Model Transforms

**Feature Branch**: `010-migration-transforms`

**Created**: 2026-09-14

**Status**: Draft

**Input**: User description: "Read .specify/intent/10-migration-transforms.md and specify this feature." — migrate data between nodes and namespaces with selectable strategies and policies; transform between datatypes and storage models; jobs observable (`08`); backup/restore jobs may invoke `13` snapshot/PITR; incompatible schema or type changes from `03` are directed here; not replica streaming (`04`).

## Clarifications

### Session 2026-09-18

- Q: Which rewrite jobs must this feature own, besides converting a container from one storage type or model to another? → A: Every rewrite other features refuse in place: type/model conversion, incompatible schema change, re-encode/recompress, re-encrypt after a key change, and sharding-key rewrite.
- Q: While a transform is rewriting a container into a new one, may clients keep writing to the source? → A: Source stays writable. Dual-write or catch-up so swap includes every durable acknowledgement (same idea as live migrate).
- Q: If a copy, move, or transform would use a container name that already exists in the destination namespace, what must happen? → A: Refuse the job, naming the existing container. The operator must drop it or choose another name.
- Q: After a namespace move cutover, what happens to the source container? → A: Dropped: the name is gone and reusable, storage is released.
- Q: Destination quota can fail at job start, or later if live writes grow the data before cutover. When must a copy, move, or transform be refused for quota? → A: Check at start and again before cutover. If it no longer fits, the job fails named; source intact; target not swapped in.
- Q: How are source fields mapped onto a transform destination when the types are not a simple pair? → A: Understandable types use a catalog mapping. Complex types require a mapping query/job that names source columns/containers and destinations.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Migrate a container between nodes or drives (Priority: P1)

A cluster administrator moves a container (or some of its replicas) to other nodes or drives under a chosen strategy. Default strategy: **live copy with dual-write then cutover** — new replicas are built while the container serves, then traffic cuts over when constraints are satisfied (`04` anti-affinity still applies). Progress is trackable (`09` / job series `08`). Requires `MIGRATE` (`14`). Automatic constraint repair remains `04`; this is an explicit, policy-driven move.

**Why this priority**: Operators must evacuate a host without dump/reload outside SpaceStorage.

**Independent Test**: Two-node then three-node: migrate a KV container off node A onto node B; confirm data identical, A holds no residual replica unless RF still places one there.

**Acceptance Scenarios**:

1. **Given** a container on node A, **When** an admin migrates it to node B with live copy + cutover, **Then** after completion B holds the data, clients read the same logical content, and the placement description matches the policy.
2. **Given** anti-affinity that B would violate, **When** migrate is requested, **Then** it is refused naming the constraint (`04`).
3. **Given** a running migration, **When** progress is queried, **Then** bytes/objects copied, remaining, and job status are available (`08`).
4. **Given** node A failure mid-migration, **When** the job resumes, **Then** it continues from recorded state without duplicating committed logical keys (idempotent apply) or it fails naming the state; it does not silently lose acknowledged source data.

---

### User Story 2 - Migrate between namespaces (Priority: P1)

A tenant (or admin) copies or moves data into another namespace under policy. Cross-namespace restore/migrate requires admin (`14`) unless both namespaces grant `MIGRATE` to the same principal under documented policy. Default: copy. Move is copy then **drop the source** after cutover (name gone and reusable, storage released).

**Why this priority**: Tenants move; constitution namespaces are isolation boundaries.

**Independent Test**: Copy a table from `acme` to `beta` as admin; confirm `beta` content matches and `acme` still has the table. Move the same shape; confirm `acme.t` is dropped (name reusable) and `beta.t` holds the data. Confirm a namespace-bound principal without `MIGRATE` is refused.

**Acceptance Scenarios**:

1. **Given** container `t` in `acme`, **When** an authorized principal migrates a copy to `beta`, **Then** `beta.t` exists with the same logical data and `acme.t` still exists if the policy was copy.
2. **Given** policy move, **When** cutover completes, **Then** `acme.t` is dropped (name gone and reusable, storage released) and `beta.t` holds the data.
3. **Given** a principal without `MIGRATE` / admin, **When** they request cross-namespace migrate, **Then** it is refused.
4. **Given** quota in `beta` that cannot hold the data, **When** migrate is requested, **Then** it is refused naming the quota (`07`) before any copy starts.
5. **Given** `beta.t` already exists, **When** a copy or move of `acme.t` to `beta.t` is requested, **Then** the job is refused naming `beta.t`; `acme.t` and `beta.t` are unchanged.
6. **Given** a running copy or move that fitted at start, **When** live writes grow the data so `beta` quota no longer holds it, **Then** cutover is refused, the job fails naming the quota, `acme.t` is intact, and any partial `beta` target is marked incomplete and not tenant-visible.

---

### User Story 3 - Rewrite a container (type, schema, codec, key, or sharding key) (Priority: P1)

An operator runs a **transform**: a background rewrite into a **new** container, then an optional name swap. The source stays **readable and writable** until swap. New durable writes are dual-written or caught up so swap includes every durable acknowledgement (same idea as live migrate). Swap MUST NOT run until the last applied source sequence is on the target. This feature owns every rewrite that `03`/`04` refuse in place:

- datatype or storage-model conversion (for example a document store into a relational table, or another L2/L3/L4 pair the catalog allows);
- incompatible schema change (drop/rename field, narrow a domain, change a field type, change a key or vector dimension);
- re-encode and/or recompress existing data after a forward-only codec change;
- re-encrypt existing data after a data-key change (`14`);
- sharding-key rewrite (changing a key `04` marks fixed after creation).

Requires a mapping. **Understandable types** (catalog-declared default mapping, or a same-type rewrite that keeps the schema: re-encode, re-encrypt, sharding-key) run with that catalog mapping — no extra query. **Complex types** (no catalog default, schema-free or nested data, several source containers, or a non-default column layout) MUST use a **mapping query** on the job: named source containers and columns/field paths, and named destination containers and columns. Impossible or incomplete mappings are refused with the reason. Codec, key, and sharding-key rewrites use the catalog's existing schema unless the job also declares a schema/type change or a mapping query.

**Why this priority**: Multiparadigm product without external dump/reload; otherwise `03`/`04` pointers have no owner.

**Independent Test**: Write documents with a schema that maps to columns; transform to `Relational Table` while further writes continue; query via SQL after swap; confirm row count and field values include those writes. Separately: drop a field via transform; recompress; re-encrypt after a key change; rewrite a sharding key — each leaves the source serving until optional swap. For a schema-free or multi-container source, submit a mapping query naming source columns/containers and destinations; confirm mapped values. Omit the query on a complex type and confirm refusal.

**Acceptance Scenarios**:

1. **Given** a `Document Store` with a documented mapping to `Relational Table`, **When** transform runs, **Then** the new table contains the mapped rows and the job completes.
2. **Given** documents that cannot map (conflicting types for one field), **When** transform runs, **Then** it fails naming the documents/fields; the source is unchanged.
3. **Given** an incompatible in-place schema change (`03`), **When** it is submitted, **Then** it is refused pointing at this transform mechanism; **When** the same change is submitted as a transform with a mapping, **Then** the new container has the new schema and mapped values.
4. **Given** optional swap, **When** it completes, **Then** the old name refers to the rewritten container (new type, schema, codec, key, and/or sharding key as requested) and the previous container is retained or dropped per policy.
5. **Given** a container whose compression or encoding was changed forward-only (`03`), **When** a re-encode/recompress transform completes, **Then** existing data is readable under the new codec only and the description lists a single codec.
6. **Given** a data-key change (`14`) with old keys retained, **When** a re-encrypt transform completes, **Then** existing data is readable only under the new key reference and the description lists that single reference.
7. **Given** a request to change a sharding key that `04` marks fixed, **When** it is submitted in place, **Then** it is refused pointing here; **When** a sharding-key transform completes, **Then** rows are placed by the new key and the source is unchanged until optional swap.
8. **Given** a running transform, **When** clients keep writing to the source, **Then** each durable acknowledgement is present on the target at swap; swap is refused until the last applied source sequence is there.
9. **Given** a transform whose new-container name already exists in the namespace, **When** the job is requested, **Then** it is refused naming that container; the source is unchanged. Optional swap of the **source's own name** is not a collision: it is the documented atomic rename onto that name after the new container was created under a free name.
10. **Given** destination quota that cannot hold a second copy at start, **When** transform is requested, **Then** it is refused naming the quota; **When** live writes grow past quota before swap, **Then** swap is refused, the job fails naming the quota, the source is intact, and the new container is marked incomplete and not swapped in.
11. **Given** an understandable type pair with a catalog mapping (or a same-type re-encode/re-encrypt/sharding-key rewrite), **When** transform runs with no mapping query, **Then** the job uses the catalog mapping and completes.
12. **Given** a complex source (no catalog mapping, schema-free/nested data, or more than one source container), **When** transform is requested without a mapping query, **Then** it is refused naming that a mapping query is required.
13. **Given** a mapping query that names source containers and columns/field paths and destination containers and columns, **When** transform runs, **Then** destination values match the query; **When** a named source column or container is missing, **Then** the job is refused naming it.

---

### User Story 4 - Backup and restore jobs invoking durability (Priority: P2)

Backup and restore **jobs** are named here and in `08`. Snapshot/PITR **mechanics** are `13`. This feature schedules the job, records policy (full snapshot, optional PITR position), and invokes `13`. Restore to same cluster or new cluster as `13` specifies. Encrypted containers produce encrypted backups (`14` keys).

**Why this priority**: Intent listed backup/restore as jobs this feature owns at the behavior-orchestration layer.

**Independent Test**: Snapshot a namespace, drop a container, restore; confirm content. PITR to a WAL position (`13`).

**Acceptance Scenarios**:

1. **Given** a snapshot job, **When** it completes, **Then** a crash-consistent snapshot exists at a WAL position (`13`) and the job is `completed` in `08`.
2. **Given** that snapshot, **When** restore-in-place runs after a drop, **Then** the container's logical data returns.
3. **Given** restore to a new cluster, **When** it completes, **Then** the data is readable there under admin policy.
4. **Given** an encrypted container, **When** backup runs, **Then** backup files are encrypted with the same key references; missing keys make restore fail naming the reference.

---

### Edge Cases

- Complex type without a mapping query: refuse naming that a mapping query is required. Catalog mapping is used only when the catalog declares the type pair (or same-type rewrite) understandable.
- Mapping query that names a missing source container, column, or field path: refuse naming the missing item.
- Sharding-key rewrite is a transform (data rewrite), not automatic rebalance (`04`).
- Concurrent writes during live migrate or transform: dual-write or catch-up window; cutover/swap only when last applied source sequence is on the target. A dual-write failure MUST fail or pause the job named; it MUST NOT acknowledge a source write that the target cannot take.
- Destination name already exists (copy, move, or transform new-container name): refuse naming the existing container; no overwrite or merge. Restore-in-place remains `13`. Transform swap of the source's own name is the optional atomic rename, not a third-party name collision.
- Namespace move cutover drops the source container (name reusable, storage released). It MUST NOT leave an empty definition behind.
- Destination quota (`07`): refuse the job at start if the destination namespace cannot hold the data (including a second copy while source remains). Re-check before cutover/swap; if live growth no longer fits, fail the job named, leave source intact, mark the target incomplete and not tenant-visible / not swapped in.
- Job cancellation: leaves source intact; target may be partial and MUST be marked incomplete and not swapped in.
- Cross-region migrate: uses `12` streams; subject to async replication rules if policy says so.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Node-to-node migration MUST exist with selectable strategies. Default: live copy, dual-write, cutover.
- **FR-002**: Namespace-to-namespace migration MUST exist with selectable copy or move policies. If the destination name already exists, the job MUST be refused naming that container; it MUST NOT overwrite or merge. After a successful move cutover, the source container MUST be dropped (name reusable, storage released); copy MUST leave the source intact.
- **FR-003**: Transforms MUST exist as background rewrites to a new container with optional name swap. Required rewrite kinds: (1) datatype or storage-model conversion, (2) incompatible schema change, (3) re-encode and/or recompress of existing data, (4) re-encrypt of existing data after a key change, (5) sharding-key rewrite. All five share that job shape. The source MUST stay writable; the job MUST dual-write or catch up so swap includes every durable acknowledgement; swap MUST wait until the last applied source sequence is on the target.
- **FR-004**: Incompatible schema or type changes, codec/key rewrites of existing data, and changes to a sharding key marked fixed after creation MUST be refused in place by `03`/`04` and directed here; this feature MUST perform them as transforms, not in-place mutation of the live source.
- **FR-005**: Jobs MUST be specifiable for data transformation, data migration, data backup, and data restore, with names aligned to `08`.
- **FR-006**: Backup/restore jobs MUST invoke `13` snapshot/PITR mechanics; they MUST NOT invent a second snapshot format.
- **FR-007**: Progress MUST be trackable (bytes/objects, status) for `08` and `09`.
- **FR-008**: Interrupted jobs MUST resume from recorded state or fail naming the state; they MUST NOT silently lose source data that was durably acknowledged.
- **FR-009**: Placement constraints (`04`) MUST be validated before cutover; unsatisfiable migrate MUST refuse.
- **FR-010**: Cross-namespace migrate/restore MUST require admin or documented dual `MIGRATE` grants (`14`).
- **FR-011**: Quota checks (`07`) MUST run at job start for copy, move, and transform. If the destination namespace cannot hold the data — including a second logical copy while the source still exists — the job MUST be refused naming the quota before any copy starts. Quota MUST be re-checked before cutover or swap; if live growth no longer fits, the job MUST fail naming the quota, the source MUST stay intact, and the target MUST be marked incomplete and MUST NOT be swapped in or made tenant-visible.
- **FR-012**: Replica streaming and repair remain `04`; this feature MUST NOT be used as the only repair path.
- **FR-013**: A transform MUST create the new container under a name that is free in the destination namespace; a colliding new-container name MUST be refused. Optional swap MAY atomically replace the source's own name after that free name exists. Restore-in-place name reuse remains `13`.
- **FR-014**: Transforms MUST use a **catalog mapping** when the type pair (or same-type rewrite) is **understandable**: the catalog declares a default mapping, or the rewrite keeps the existing schema (re-encode, re-encrypt, sharding-key). **Complex** sources — no catalog default, schema-free or nested data, more than one source container, or a non-default column layout — MUST NOT run on a catalog mapping. They MUST accept a **mapping query** on the job that names source containers and columns/field paths and destination containers and columns. A complex transform without a mapping query MUST be refused. A mapping query that names a missing source MUST be refused. Mapping-query evaluation uses query execution (`05`); this feature owns the job and the required source/destination names.

### Key Entities

- **Migration Job**: Source, destination (nodes/drives/namespace), strategy, policy, progress, status.
- **Transform Job**: Source container(s); rewrite kind (type/model, incompatible schema, re-encode/recompress, re-encrypt, sharding-key); optional target type/model; mapping mode (catalog mapping or mapping query); optional mapping query (source containers and columns/field paths, destination containers and columns); new container id; swap flag; dual-write/catch-up progress (last applied source sequence).
- **Mapping Query**: Operator-specified source containers and columns/field paths plus destination containers and columns, required for complex transforms; evaluated through `05`.
- **Catalog Mapping**: Default field/column mapping the type catalog (`03`) declares for an understandable type pair or same-type rewrite.
- **Backup Job**: Scope (container/namespace), snapshot id, WAL position, encryption key references.
- **Restore Job**: Snapshot id, optional PITR position, destination cluster/namespace.
- **Strategy**: Live copy+cutover (default node-to-node); snapshot+restore (disaster); offline copy (documented).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of node-to-node live migrations in the suite complete with identical logical data and 0 residual replicas violating the destination policy.
- **SC-002**: 100% of copy namespace migrations leave the source intact; 100% of moves drop the source after cutover (name reusable, no empty leftover). 100% of copy/move/transform requests whose destination (or transform new-container) name already exists are refused with that name; 0 silent overwrites. 100% of start-time over-quota requests are refused before copy; 100% of cutover-time over-quota jobs fail named with source intact and 0 tenant-visible partial targets.
- **SC-003**: 100% of documented transforms in the suite produce the requested rewrite (target type/schema, single codec, single key reference, or new sharding key) with mapped values; 100% of impossible mappings fail without modifying the source. The suite MUST include at least one fixture per rewrite kind in FR-003, one understandable-type fixture with no mapping query, one complex-type fixture with a mapping query, and one complex-type fixture refused for a missing mapping query. 100% of live-write transform fixtures include every durable source acknowledgement on the target at swap.
- **SC-004**: 100% of interrupted-then-resumed jobs in the suite either complete correctly or fail named; 0 silent source data loss of durable acks.
- **SC-005**: 100% of snapshot+restore fixtures recover logical content; encrypted backups fail restore without keys in 100% of missing-key tests.
- **SC-006**: An operator following starter docs migrates a container off a node in under 20 minutes on a small dataset.

## Assumptions

- Rebalancing to restore declared RF/anti-affinity automatically is `04`; operator-initiated evacuate/move is this feature. Changing a sharding key is a transform here, not `04` rebalance.
- This feature owns every rewrite `03`/`04` refuse in place: type/model conversion, incompatible schema change, re-encode/recompress, re-encrypt after a key change, and sharding-key rewrite.
- Default transform is live: source writable, dual-write or catch-up, swap only when last applied source sequence is on the target. Offline copy remains an explicit strategy, not the transform default.
- WAL/snapshot bytes are `13`; keys are `14`; job series names are `08`; map UI is `09`.
- Default refuse-first when destination quota is insufficient at start (no copy starts). Live growth that exceeds destination quota fails the job at cutover/swap named; source intact; no tenant-visible partial target. No quota reservation protocol.
- Understandable types use a catalog mapping (or need none for same-type rewrites). Complex types require a mapping query naming source columns/containers and destinations; evaluation is `05`.
- Composition transform default is refuse (transform members first).

## Out of Scope

- Ongoing replica streaming (`04`).
- Raft membership changes (`06`) except as topology consumer.
- UI chrome (`09`) except progress MUST be trackable.
- Compaction/TTL (`13`) except as jobs that may run after transform.
