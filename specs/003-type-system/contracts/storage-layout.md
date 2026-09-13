# Contract: Storage Modes, Block Pipeline, Layouts and Restore

**Feature**: `003-type-system` | Crate: `crates/storage` | Spec: FR-021–FR-023, FR-029, FR-037, FR-038 | Clarification Q2

## 1. Storage modes

| Mode | Content location | Durability | Restart behaviour |
|---|---|---|---|
| `memory` | `types.memory` buffer only | **volatile** | definition and options restored, content **empty**, description says so (Clarification Q2) |
| `persistent` | drives under `storage.data_dir`; memory used as `storage.block_cache` | durable | definition, options and content intact |
| `hybrid` | declared portion in memory, remainder on drives | durable for the persistent part | persistent part intact; memory part rebuilt per `HybridPolicy` |

All three expose the same operation set for a given type (FR-022). A mode unsupported by the type or by any layout component is refused with the supported list (FR-021).

### Hybrid policies

| Policy | Memory portion | Used by |
|---|---|---|
| `Recent { window }` | items newer than `window` | `timeseries`, `timeseries_segment`, `log_stream` |
| `HotSet { fraction }` | most-recently-used fraction of items | `map`-family, `kv_store`, `document_store` |
| `WriteBuffer` | the `memtable` of the LSM layout | every `lsm_tree`-backed type |
| `IndexResident` | index/graph/dictionary in memory, payloads on drives | `hnsw`, `scann`, `vector_*`, `spatial_*`, `ngram_index`, `fulltext_search` |

The description states which part is where and reports the memory portion as full when exhausted; eviction follows the policy and never changes the operation set (spec edge case).

## 2. Block pipeline

Fixed order, not configurable (FR-026):

```text
values ──encode(per field/data kind)──► block assembly (target 64 KiB, `types.block_size`)
       ──compress(per block)──► ──encrypt(per block, AEAD)──► memory or drive
```

### Block header

| Field | Bytes | Notes |
|---|---|---|
| magic | 4 | `SSB1` |
| format_version | 2 | |
| encoding_id | 2 | registry id |
| codec_id | 2 | registry id |
| uncompressed_len | 4 | |
| payload_len | 4 | after compression, before encryption expansion |
| crc32 | 4 | over the **pre-encryption** payload (distinguishes corruption from a wrong key) |
| flags | 2 | encrypted, dictionary-fallback, last-block |
| *(if encrypted)* algorithm_id | 2 | |
| *(if encrypted)* key_version | 4 | |
| *(if encrypted)* nonce | 12 | random per block |

AEAD associated data = `container_id ‖ file_id ‖ block_index`, so a block cannot be replayed at another offset or in another container.

## 3. L0 storage primitives

| Primitive | Format |
|---|---|
| `memtable` | in-memory sorted structure (skip list by default, hash table for unordered types) sized by the `storage.memtable` buffer; flushed to an `sstable` on threshold or age |
| `wal` | segmented append-only log; record = length, CRC32, LSN, container id, payload; group commit with `storage.sync` ∈ `fsync \| fdatasync \| none`; truncated after the covering flush |
| `sstable` | header, data blocks (§2), optional `bloom_filter` block, sparse index block, footer with offsets and a per-file checksum; block-aligned for random reads and the block cache |
| `append_segment` | rolled segments with an offset index; supports prefix truncation (retention) and offset→time lookup |

`lsm_tree` composes `memtable` + `wal` + `sstable` (+ optional `bloom_filter`, `bplus_tree` index) with levelled compaction; compaction, index build and ANN graph build run on the bounded `spawn_blocking` pool so Tokio workers never stall (Principle II).

None of these five is directly creatable (FR-014); a create request naming one is refused with the list of types that use it.

## 4. Layout composition rules

- Every layout component must appear in the type's `composed_from` (FR-038); otherwise `LayoutComponentNotAllowed{type, primitive}`.
- Roles: `Primary`, `Index`, `Filter`, `Log`, `Cache`, `Dictionary`, `Graph`. A type declares which roles it needs; a definition may substitute an allowed primitive per role (for example `ordered_set` with `bplus_tree` instead of `skip_list`).
- Capabilities apply to the container as a whole: no component may carry its own placement, and a definition attempting it is refused (FR-033). This is structural — `LayoutComponent` has no capability field.
- Omitting `layout.*` yields the descriptor's `default_layout`, which is why an option-free create works for every type (FR-029).

## 5. On-disk arrangement

```text
<storage.data_dir>/
├── catalog/
│   ├── catalog.log                     # append-only CatalogRecord log, CRC32, fsync before ack
│   └── catalog.snapshot.<n>            # periodic full snapshot; older snapshots pruned after fsync
└── ns/<namespace>/<container-id>/
    ├── wal/<seq>.wal
    ├── sst/<level>-<seq>.sst
    ├── seg/<seq>.seg                   # append_segment based types
    ├── idx/<name>.idx                  # index and graph files (hnsw, scann, spatial, ngram)
    └── meta.json                       # redundant copy of the definition for forensics (never authoritative)
```

Container directories are named by `ContainerId`, so a rename is a catalog-only operation and compositions referencing the id are unaffected.

## 6. Boot restore (FR-023, Principle XII)

```text
1. open catalog: newest valid snapshot, then replay catalog.log tail (skip a torn trailing record)
2. rebuild ContainerCatalog: definitions, schemas, capabilities, compositions, dependants
3. per container, by mode:
     memory      → state RestoredEmpty; content not restored; description states it
     persistent  → replay WAL past the last flushed LSN; open SSTables/segments; state Ready
     hybrid      → persistent part as above; memory portion rebuilt per HybridPolicy; state Ready
4. per container, exceptions:
     key unresolvable → Unavailable{KeyUnresolvable}; no plaintext access; nothing discarded (FR-027)
     unknown type     → Unavailable{UnknownType}; reported in the per-node catalog diff (FR-011)
     corrupt block    → container Ready, block quarantined, error recorded; never silently dropped
5. compositions: resolve members by id; missing member → Degraded{missing} per the kind's tolerance
6. node reports ready only after step 3 completes for every container
```

Restore of 10 000 container definitions targets < 30 s; per-container WAL replay is parallel across containers on the blocking pool.

## 7. Buffers registered by this feature

| Buffer | Default | Range | Policy | Purpose |
|---|---|---|---|---|
| `types.memory` | 1 GiB | 16 MiB – 1 TiB | Reject | memory-mode content and hybrid memory portions (re-homed from `002`) |
| `storage.memtable` | 256 MiB | 4 MiB – 256 GiB | Wait | write buffers across all LSM containers |
| `storage.block_cache` | 512 MiB | 0 – 1 TiB | Reject (LRU evict) | decoded block cache for persistent reads |
| `storage.wal` | 64 MiB | 1 MiB – 16 GiB | Wait | group-commit staging |
| `catalog.metadata` | 32 MiB | 4 MiB – 4 GiB | Reject | catalog records and snapshots in memory |

Usage is reported per buffer by `001`'s existing buffer machinery and is live-reloadable.
