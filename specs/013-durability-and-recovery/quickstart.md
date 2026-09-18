# Quickstart: Durable ack, crash restore, snapshot/PITR

**Feature**: `013-durability-and-recovery` | **Gates**: SC-001–SC-009 | Slice 2 (WAL/restore) and slice 10 (operator snapshot)

Prerequisites: `001` runtime; `003` KV/persistent container; `014` keys if encrypting. Model: [data-model.md](data-model.md). Commands: [admin-cli.md](contracts/admin-cli.md).

Configs: [single-drive.conf](contracts/fixtures/single-drive.conf), [two-drive.conf](contracts/fixtures/two-drive.conf).

## 1. Durable `TWO` survives kill (SC-001)

Start [single-drive.conf](contracts/fixtures/single-drive.conf). Create a persistent `kv_store`. Write at `ONE` (one durable WAL). `spacestorage storage` shows `durable_lsn` advanced. Kill -9. Restart. Read returns the value. Three-node `TWO` uses the `012`/`016` harness: crash the two acknowledging replicas; both restore the value (SC-001).

[sync-none.conf](contracts/fixtures/invalid/sync-none.conf) → startup `sync_none_not_durable`.

## 2. Memory-mode stays volatile (SC-002)

Create memory-mode container, write, restart. Definition remains; content empty; description states volatility. Snapshot of a namespace that includes it stores the definition only (SC-009).

## 3. Disk-full is per drive (SC-003)

Start [two-drive.conf](contracts/fixtures/two-drive.conf). Pin container A to `nvme0`, B to `hdd0`. Fill `nvme0`. Write A → `disk_full`; write B still durable; `node_state=degraded`; prior A acks still readable.

## 4. Format and torn tail (SC-004)

Present a WAL major 99 → node refuses start. Truncate the last WAL byte → start succeeds; torn record skipped (`db_wal_torn_total`).

## 5. Tombstones (SC-005)

Delete a key; run compaction immediately → tombstone retained. After `gc_grace` (test override, e.g. 1s) and local replica seen → compaction MAY drop.

## 6. Snapshot and PITR (SC-006, SC-007, SC-008) — library in slice 2, jobs in slice 10

Snapshot namespace → manifest has a position **per drive**. Restore without drop onto a live container → `RestoreDestinationHasContent`. Drop (or `--confirm-drop`) → fill. PITR with HLC `T` applies each drive through last stamp ≤ `T`. Unmappable drive → job refused, snapshot data not partially applied. Encrypted snapshot without key → fail naming the ref (SC-007).
