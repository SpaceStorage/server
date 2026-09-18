# Contract: Snapshot

**Feature**: `013-durability-and-recovery` | Crate: `crates/backup` | Spec: FR-013 | Jobs: `008` `snapshot` / `backup`

## Cut

Record `{ drive_id → lsn }` for involved drives **without** pausing other drives. Per-drive crash-consistency only. A multi-drive snapshot MUST NOT be described as one cluster instant.

## Contents

- Definitions and options of every in-scope container.
- Persistent and hybrid **content** as of that drive’s LSN (hardlink if same fs, else copy).
- **Not** memory-mode content.

Scope: one container or one namespace (`010` job). Full snapshot required; incremental optional later.

## Encryption

If source containers are encrypted, snapshot files use the **same key references** (`014`). Stolen snapshot without keys is ciphertext.

## Artifact

`{data_dir}/snapshots/<snapshot_id>/manifest.json` + data dirs. Manifest fields: [data-model.md](../data-model.md) `SnapshotManifest`.

`010` stores the **position map**. A single LSN is insufficient when scope spans drives.

## Pins

A snapshot pins WAL/SSTables it needs for PITR until the snapshot is deleted or expire policy runs (`GC` job respects pins).
