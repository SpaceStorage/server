---
speckit_command: specify
suggested_slug: durability-and-recovery
source: gap-analysis-2026-09-14
read_after: 00-constitution.md
---

# Feature: Durability, WAL, crash recovery, deletes, TTL, compaction, and backup

Specify what an acknowledgement means, how a node recovers after crash, how deletes and TTL reclaim space, and backup/restore as a product.

## What

### Acknowledgements and WAL

For **persistent** and **hybrid** containers, a replica acknowledgement that **counts toward write quorum** (`04`, `12`) MUST mean the write is recoverable on that node after a process crash: it is in a write-ahead log (WAL) that has been made durable (fsync or equivalent group commit). Group commit is allowed. Memory-resident structures alone MUST NOT count as a durable ack for these containers.

For **memory-mode** containers, a counted acknowledgement is memory-only and MUST NOT be claimed as crash-durable. Mixed replica sets MUST report which acknowledgements were durable (`04`).

WAL is **one stream per node (or per drive)**. Each record MUST be encrypted with that **container's data key** when the container is encrypted (`14`). Unencrypted containers yield **plaintext WAL records** (operator-chosen leak). WAL MUST have rotation, retention (at least long enough for replica catch-up under documented repair retention and `gc_grace`), and replay on start. Checkpoints MAY truncate prefix of WAL that is covered by a consistent on-disk image.

Blocking durability syscalls MUST run **off the async worker pool** so Tokio workers stay non-blocking (constitution). Database request code remains async; fsync is scheduled, not executed on a worker thread.

### Crash recovery and restore

When a node starts it MUST:

1. Restore **definitions and options** of every local container (always).
2. Restore **content** of persistent and hybrid containers from drives (WAL replay + checkpoints / SSTables / whatever the layout uses).
3. Restore memory-mode containers **empty** of content unless replication (`04`) re-populates them. Memory mode is a volatile tier. The description MUST say so.

This is the meaning of constitution "restore state of all local datatypes": definitions always; durable content from media; memory content not promised across restart.

Corrupt files MUST be isolated (not used as truth) and the node MUST attempt to rebuild that part from a replica or report the container degraded. Disk-full MUST refuse new durable writes, keep reads that can be served, and set `node_state` degraded; it MUST NOT crash-loop or silently drop acks.

### On-disk format version

Every durable file and WAL segment MUST carry a **format version**. A node MUST refuse to start on a major version it does not understand. Rolling upgrade: a cluster MAY mix adjacent product versions N and N+1 (`15`); an old node MUST refuse to write a newer format.

### Deletes, tombstones, TTL, compaction

- A delete is visible to subsequent reads at the requested quorum once enough replicas have applied the tombstone (or equivalent).
- **`gc_grace`:** compaction MAY drop a tombstone only after (1) every replica **in the source `quorum_domain`** has seen it (or is rebuilt from a snapshot newer than the delete) **and** (2) a documented grace interval covering repair/WAL retention. Default grace is documented (hours-scale, namespace/container override). Async remotes consume deletes via the **source log** (`12`); the source MUST NOT compact away a tombstone still required by a remote that is not caught up, unless that remote rebuilds from snapshot.
- TTL: if the type has an event-time field, expiration uses that; otherwise ingest/HLC time **in the source domain**. Expired values MUST become deletes (tombstones) via background jobs (`08` job name `TTL expiration`).
- Compaction, flush, checkpoint, vacuum, GC (`08` background jobs) have **this feature** as behavior owner. They MUST be observable as those jobs. Per-type default compaction strategy is documented in the type catalog (`03`).

Null (typed empty), missing field, and tombstone (deleted) are three different states (`03` value domains).

### Backup and restore (product, not only a metric)

This feature owns backup/restore **behavior**; `10` owns migration/transform jobs that may call into it; `08` owns series names.

- **Snapshot** MUST be crash-consistent at a WAL position (all durable acks up to that position).
- **PITR**: restore MAY apply WAL after the snapshot up to a requested timestamp / WAL position, not past the last durable ack.
- Incremental backups MAY exist; full snapshot MUST exist.
- Restore into the **same** cluster (replace) or a **new** cluster (disaster recovery) MUST both be specified procedures.
- Backup files MUST be encrypted when the source containers are encrypted, using the same key references (`14`); key loss makes backup unrestorable and that MUST be explicit.
- Cross-namespace restore is allowed only with admin authorization (`14`).
- RPO for a persistent container acknowledged at the requested write quorum is **zero durable acks lost** on the replicas that acknowledged (constitution-adjacent; `04` SC for acks). RTO is operational (restore job duration), observable (`08`).

WAL shipping as continuous backup MAY be a later addition; first-binary snapshot + WAL retain is enough if stated. Restore MUST accept a **specified key** (operator-supplied key reference / material per `14`) when re-encrypting or unlocking snapshots; this MUST be documented.

## Why

Quorum without a durability contract lies to clients. Deletes without tombstone rules resurrect data. A planetary database that cannot backup or upgrade on-disk format cannot be operated.

## Actors

- Coordinator counting durable vs memory acks
- Node replaying WAL at boot
- Compaction / TTL background jobs
- Operator taking a snapshot and restoring a cluster
- Replica catch-up consuming retained WAL

## Requirements

- Durable ack = WAL made durable for persistent/hybrid; memory ack for memory-mode; mixed sets report which. Write quorum **which nodes count** is `12`.
- One WAL per node/drive; per-record encryption with the container data key.
- `gc_grace` tombstone drop rule as specified.
- Fsync off the async worker pool.
- Restore rules: definitions always; durable content from disk; memory content volatile.
- Format version on durable files; refuse unknown major; N/N+1 mixed-version with `15`.
- Tombstones, TTL clock, compaction owned here; three-way null/missing/tombstone with `03`.
- Crash-consistent snapshot + PITR from WAL; encrypted backups; same-cluster and new-cluster restore.
- Disk-full and corruption behavior as specified.

## Out of scope for this feature

- Quorum arithmetic and leaderless fan-out (`04` / `12`)
- Key hierarchy and KMS (`14`)
- Migration between datatypes (`10`) except calling backup/restore
- Metric series names (`08`)
- Membership (`11`)
