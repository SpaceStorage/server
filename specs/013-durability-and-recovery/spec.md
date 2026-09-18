# Feature Specification: Durability, WAL, Restore, Deletes, TTL, Compaction, and Backup

**Feature Branch**: `013-durability-and-recovery`

**Created**: 2026-09-15

**Status**: Draft

**Input**: User description: "Read .specify/intent/13-durability-and-recovery.md and specify this feature." — durable ack = WAL made durable for persistent/hybrid; memory ack for memory-mode; one WAL per drive that holds persistent/hybrid data; per-record encryption with container data key; restore definitions always / durable content from media / memory volatile; `gc_grace`; snapshot + PITR; fsync off worker pool.

## Clarifications

### Session 2026-09-15

- Q: What does a counted write acknowledgement mean for persistent data? → A: The write is in a WAL that has been made durable (fsync or equivalent group commit). Memory-resident structures alone MUST NOT count for persistent/hybrid containers.
- Q: One WAL or many? → A: One stream **per drive** that holds persistent or hybrid data (the 2026-09-18 session closed the earlier "per node or per drive" fork). Each record encrypted with that container's data key when the container is encrypted (`14`). Unencrypted containers yield plaintext WAL records (operator-chosen leak).
- Q: When may compaction drop a tombstone? → A: After every replica in the source `quorum_domain` has seen it (or is rebuilt from a snapshot newer than the delete) **and** a documented `gc_grace` interval. Async remotes consume deletes via the source log (`12`); the source MUST NOT compact away a tombstone still required by a remote that is not caught up, unless that remote rebuilds from snapshot.

### Session 2026-09-18

- Q: When a node has more than one drive that stores persistent or hybrid data, how many write-ahead logs should that node keep? → A: One log on each drive that holds persistent or hybrid data.
- Q: When a snapshot covers containers whose durable copies on a node sit on more than one drive, how should that snapshot be crash-consistent? → A: Independent per drive: record each drive's WAL position without pausing the others; crash-consistency is per drive only.
- Q: When restoring a snapshot into the same cluster under existing container names, what must already be true of those live containers? → A: Refused while the destination still has content; the operator drops first, or the restore job drops only after an explicit confirm, then fills the names.
- Q: Should a snapshot include the current contents of memory-mode containers? → A: Never. Definitions and options of in-scope memory-mode containers are included; content is omitted and stays volatile.
- Q: When an operator asks to restore "to a point in time" after a snapshot, what identifier should PITR actually use? → A: Timestamp is source-domain HLC (or ingest time); map to the last WAL position on each involved drive with stamp ≤ that HLC; refuse if unmappable.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Durable acknowledgements (Priority: P1)

A client writes to a persistent container at `TWO`. The coordinator does not acknowledge until two source-domain replicas (`12`) have durable WAL copies on the drive that holds that replica's copy. Each drive that holds persistent or hybrid data has its own WAL; group commit is per that drive's log. Fsync runs off the worker pool. A memory-mode write acknowledges memory only and MUST NOT be claimed crash-durable.

**Why this priority**: Quorum without durability lies to clients.

**Independent Test**: Write persistent at `TWO`; kill the process; restart; read the value. Write memory-mode; restart; confirm empty unless replicated.

**Acceptance Scenarios**:

1. **Given** a persistent container, **When** a write is acknowledged at a counted level, **Then** a crash and restart of those acknowledging replicas still returns the value after WAL replay.
2. **Given** a memory-mode container, **When** a write is acknowledged, **Then** the description MUST NOT claim crash-durability; restart without replicas loses content (`03`).
3. **Given** mixed replica sets, **When** a write completes, **Then** the execution record distinguishes durable vs memory acknowledgements (`04`).
4. **Given** fsync, **When** it runs, **Then** it does not occupy a database worker thread.
5. **Given** a node with two drives that each hold persistent or hybrid data, **When** a write is durably acknowledged for a container on drive 1, **Then** that acknowledgement is a durable record in drive 1's WAL and MUST NOT require a durable record in drive 2's WAL.

---

### User Story 2 - Crash recovery and disk failure (Priority: P1)

On start: restore definitions and options always; restore persistent/hybrid content from drives (WAL replay + checkpoints); restore memory-mode content empty unless replication re-populates. Corrupt files are isolated. Disk-full on a drive refuses new durable writes for containers on that drive, keeps readable data, sets degraded, and leaves other drives able to take durable writes; no crash-loop and no silent drop of acks. Durable files and WAL segments carry a format version; unknown major refuses start; old nodes refuse to write newer format (`15`).

**Why this priority**: Constitution restore split.

**Independent Test**: Restart with data; truncate a file; fill the disk; present an unknown major version.

**Acceptance Scenarios**:

1. **Given** persistent data and a restart, **When** the node is `ready`, **Then** definitions, options, and content are present.
2. **Given** unreplicated memory-mode data and a restart, **When** the node is `ready`, **Then** definition remains, content is empty, description states volatility.
3. **Given** a corrupt file, **When** the node starts, **Then** that part is not used as truth; the node rebuilds from a replica or reports the container degraded.
4. **Given** disk-full on one drive, **When** a new durable write targets a container whose copy on this node lives on that drive, **Then** it is refused; durable writes for containers on other drives continue; reads that can be served continue; `node_state` is degraded; previously counted acks are not silently dropped.
5. **Given** an unknown major format, **When** the node starts, **Then** it refuses to start. **Given** mixed N/N+1, **When** an old node would write a newer format, **Then** it refuses.

---

### User Story 3 - Deletes, TTL, compaction (Priority: P2)

A delete is visible once enough replicas applied the tombstone. Compaction drops tombstones only after `gc_grace` rules. TTL uses event-time if the type has it, otherwise ingest/HLC time in the source domain; expired values become deletes via the `TTL expiration` job (`08`). Null, missing field, and tombstone are distinct (`03`).

**Why this priority**: Deletes without tombstone rules resurrect data.

**Independent Test**: Delete; compact too early (must retain); wait grace; compact; TTL expire.

**Acceptance Scenarios**:

1. **Given** a delete at the requested quorum, **When** a subsequent read at that quorum runs, **Then** the value is absent (tombstone visible).
2. **Given** a tombstone still required by a lagging source-domain replica or an uncaught-up follower, **When** compaction runs, **Then** the tombstone is retained.
3. **Given** grace elapsed and all required replicas have seen the delete, **When** compaction runs, **Then** the tombstone MAY be dropped.
4. **Given** TTL, **When** a value expires, **Then** it becomes a tombstone via the background job, not a silent omit.

---

### User Story 4 - Snapshot and PITR (Priority: P2)

An operator takes a snapshot that records a WAL position **per involved drive** without pausing other drives. Crash-consistency is **per drive only**: a container whose copy on a node lives on one drive is crash-consistent at that drive's recorded position; a snapshot that spans several drives MUST NOT be claimed as one cluster-wide instant. Snapshots include **persistent and hybrid content** plus **definitions and options** of every in-scope container. **Memory-mode content is never included**; those containers restore empty of content unless replication (`04`) re-populates them. Restore MAY then apply that drive's WAL up to a requested **per-drive WAL position** or a **source-domain HLC / ingest timestamp**. A timestamp maps on each involved drive to the last durable WAL record whose stamp is ≤ that HLC; a drive that cannot map MUST fail the PITR job naming that drive. PITR MUST NOT pass the last durable ack on that drive. Event-time fields used for TTL are not the PITR clock. Full snapshot MUST exist; incremental MAY. Restore into the same cluster or a new cluster. Same-cluster restore under existing names is **refused while a destination still has content**; the operator drops that destination first, or the restore job drops it only with an **explicit confirm**, then fills the names. Encrypted containers produce encrypted backups; restore accepts a specified key (`14`). Lost key without backup ⇒ encrypted data unrestorable, and that is explicit. Cross-namespace restore requires admin (`14`).

**Why this priority**: Intent requires backup as a product, not only a metric.

**Independent Test**: Snapshot; drop; restore. PITR to a WAL position and to an HLC timestamp. Encrypted snapshot without key fails named. Snapshot a memory-mode container; restore empty content.

**Acceptance Scenarios**:

1. **Given** a snapshot job, **When** it completes, **Then** it records a WAL position for each involved drive covering all durable acks on **that drive** up to that position, and it MUST NOT pause other drives to form a single instant.
2. **Given** PITR by WAL position, **When** restore applies WAL after the snapshot, **Then** it stops at the requested position **on that drive** and MUST NOT pass the last durable ack on that drive.
2a. **Given** PITR by a source-domain HLC (or ingest) timestamp, **When** each involved drive can map it, **Then** restore applies that drive's WAL through the last durable record whose stamp is ≤ that HLC and no further. **Given** a drive that cannot map the timestamp, **When** the job runs, **Then** PITR is refused naming that drive; no drive is applied past the snapshot under that failed job.
3. **Given** restore to a new cluster vs in-place, **When** each procedure is followed, **Then** data is readable under the documented policy.
3a. **Given** a same-cluster restore whose destination container still has content, **When** the job has no explicit confirm-to-drop, **Then** it is refused naming the destination and content is left intact. **Given** the same destination after a drop, or with explicit confirm-to-drop, **When** restore runs, **Then** the name is filled from the snapshot and is readable.
3b. **Given** a new-cluster restore whose destination name already has content, **When** submitted without confirm-to-drop, **Then** it is refused the same way.
4. **Given** encrypted source, **When** backup runs, **Then** files are encrypted with the same key references; restore with a specified key is documented; missing keys fail naming the reference.
5. **Given** WAL shipping, **When** considered for the first binary, **Then** snapshot + WAL retain is enough; continuous WAL shipping MAY be later.
6. **Given** a snapshot whose containers on one node span two drives, **When** it completes, **Then** the two recorded positions MAY correspond to different wall times; the snapshot MUST NOT be described as one crash-consistent cluster cut.
7. **Given** an in-scope memory-mode container, **When** a snapshot completes, **Then** the snapshot holds its definition and options and MUST NOT hold its content. **Given** restore of that snapshot, **When** the memory-mode container is created, **Then** it is empty of content unless replication re-populates it, and the description still states volatility.

---

### Edge Cases

- Group commit batches several writes into one durable operation on that drive's WAL; each counted ack still means recoverability.
- Unencrypted WAL records are a chosen leak (`14` Q13-B).
- A node with a single persistent/hybrid drive has exactly one WAL (the per-drive rule with one drive).
- A snapshot spanning several drives is not one instant: restored namespace/cluster state MAY include writes that never coexisted across drives. Cross-container atomicity is not a snapshot promise (`05` transactions are a separate cut).
- Same-cluster (and new-cluster) restore MUST NOT overwrite a destination that still has content. Drop is operator-initiated or an explicit confirm on the restore job; live writes to those names are gone before fill begins.
- Memory-mode content is absent from snapshots; restoring it does not resurrect pre-snapshot memory contents.
- PITR timestamp is source-domain HLC or ingest time, not operator wall clock and not a type's event-time TTL field. Mapping is per drive; any unmappable drive fails the whole PITR job.
- RPO for a persistent container acknowledged at the requested write quorum is zero durable acks lost on replicas that acknowledged. RTO is operational, observable (`08`).

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: For persistent and hybrid containers, a counted write ack MUST mean WAL durable (fsync or equivalent group commit). Memory structures alone MUST NOT count.
- **FR-002**: For memory-mode, a counted ack is memory-only and MUST NOT be claimed crash-durable. Mixed sets MUST report which acks were durable.
- **FR-003**: WAL MUST be one stream **per drive** that holds persistent or hybrid data. A node with several such drives MUST keep several WALs (one per those drives), not one shared node-wide log and not one log per container or namespace. Rotation and retention MUST cover replica catch-up and `gc_grace`. Each drive's WAL is replayed on start. Checkpoints MAY truncate a covered prefix of **that drive's** WAL.
- **FR-004**: WAL records of encrypted containers MUST be encrypted with that container's data key (`14`). Unencrypted containers yield plaintext records.
- **FR-005**: Blocking durability syscalls MUST run off the async worker pool.
- **FR-006**: On start: restore definitions/options always; persistent/hybrid content from drives; memory-mode content empty unless replication re-populates.
- **FR-007**: Corrupt files MUST be isolated; rebuild from replica or degrade. Disk-full on a drive MUST refuse new durable writes for containers whose copy on this node lives on that drive, keep servable reads, set degraded, and MUST NOT refuse durable writes for containers on other drives that still have room; MUST NOT crash-loop or silently drop acks.
- **FR-008**: Every durable file and WAL segment MUST carry a format version. Unknown major → refuse start. Old node MUST NOT write a newer format (`15`).
- **FR-009**: `gc_grace` tombstone drop rule as specified (source-domain visibility + grace; followers via source log).
- **FR-010**: TTL uses event-time if present, else ingest/HLC in the source domain; expiry becomes tombstones via the named background job.
- **FR-011**: Compaction, flush, checkpoint, vacuum, GC behavior is owned here; observability names are `08`; per-type default strategy is in the catalog (`03`).
- **FR-012**: Null, missing field, and tombstone are three states (`03`).
- **FR-013**: Snapshot MUST record a WAL position per involved drive and MUST be crash-consistent **per drive** (all durable acks on that drive up to that position). It MUST NOT pause other drives to manufacture a single cluster instant. Snapshot MUST include persistent and hybrid content for in-scope containers plus definitions and options of every in-scope container. Snapshot MUST NOT include memory-mode content. PITR MAY apply that drive's WAL after the snapshot up to a requested per-drive WAL position or a source-domain HLC/ingest timestamp, not past last durable ack on that drive. A timestamp MUST map on each involved drive to the last durable WAL record whose stamp is ≤ that HLC; if any involved drive cannot map, PITR MUST be refused naming that drive. TTL event-time MUST NOT be used as the PITR clock. Full snapshot MUST exist.
- **FR-014**: Restore into same cluster or new cluster MUST both be documented procedures. Restore MUST be refused while a destination container still has content, unless the job carries an explicit confirm to drop that destination first; after drop (or when the name has no content), restore fills the names. Cross-namespace restore requires admin (`14`). Restore-in-place name reuse after drop is this feature; `10` MUST NOT invent a second overwrite path.
- **FR-015**: Backups of encrypted containers MUST use the same key references. Restore with a specified key MUST be documented. Lost master/data key without backup ⇒ those encrypted containers unrestorable, stated explicitly.
- **FR-016**: Which nodes count toward write quorum remains `12`.

### Key Entities

- **WAL**: Per-drive durable log on each drive that hosts persistent or hybrid data; records keyed by container data key when encrypted.
- **Durable Ack**: WAL made durable on that replica.
- **Checkpoint**: On-disk image allowing WAL prefix truncate.
- **Tombstone / `gc_grace`**: Delete retention.
- **Snapshot / PITR**: Backup and restore keyed by per-drive WAL positions; optional PITR timestamp is source-domain HLC/ingest mapped per drive; crash-consistency is per drive, not a cluster-wide instant.
- **Format Version**: On-disk compatibility with `15`.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of persistent writes acknowledged at `TWO` in the suite survive crash+restart of the acknowledging replicas.
- **SC-002**: 100% of unreplicated memory-mode containers in the suite come back empty after restart with a volatility notice.
- **SC-003**: 100% of disk-full tests refuse new durable writes for containers on the full drive without dropping prior counted acks, and 100% of those tests still accept a durable write for a container on a second drive that is not full.
- **SC-004**: 100% of unknown-major starts refuse; 100% of N nodes refuse to write N+1 formats.
- **SC-005**: 100% of premature compaction tests retain tombstones still required by source-domain replicas or uncaught-up followers.
- **SC-006**: 100% of snapshot restores in the suite return data acknowledged on that drive before the recorded position for that drive; PITR never applies past last durable ack on that drive; 100% of multi-drive snapshot fixtures record independent per-drive positions without a cluster-wide freeze. 100% of HLC-timestamp PITR fixtures apply each mappable drive through the last stamp ≤ the requested HLC; 100% of unmappable-drive PITR jobs are refused naming the drive.
- **SC-007**: 100% of encrypted backups fail restore without the specified key, naming the reference.
- **SC-008**: 100% of same-cluster restore attempts onto a destination that still has content are refused with the destination intact unless the job carries explicit confirm-to-drop; 100% of restores after drop (or confirm-drop) fill those names from the snapshot.
- **SC-009**: 100% of snapshots that include a memory-mode container omit its content; 100% of restores of those snapshots leave that container empty of content unless a replica re-populates it.

## Assumptions

- Write-quorum **who counts** is `12`. Placement repair windows are `04`.
- Key hierarchy is `14`. Migration jobs MAY invoke snapshot (`10`).
- First binary: snapshot + WAL retain; continuous WAL shipping later (`16`).
- Default `gc_grace` is hours-scale with namespace/container override; exact default is a planning decision.

## Out of Scope

- Quorum arithmetic and leaderless fan-out (`04`/`12`).
- Key hierarchy and KMS (`14`).
- Migration between datatypes (`10`) except calling backup/restore.
- Metric series names (`08`).
- Membership (`11`).
