# Contract: Configuration Directives Added by This Feature

**Feature**: `003-type-system` | Extends [`001` config grammar](../../001-runtime-cli-api/contracts/config-grammar.md) and [`002` directives](../../002-protocol-drivers/contracts/config-directives.md) | Implemented in `crates/config` (feature-registered blocks)

## New blocks

| Path | Arity | Type | Default | Reload | Notes |
|------|-------|------|---------|--------|-------|
| `storage { data_dir P; }` | 1 | PATH | — | restart | Mandatory once any persistent or hybrid container can exist; created if absent, must be writable |
| `storage { sync M; }` | 1 | `fsync \| fdatasync \| none` | `fdatasync` | **live** | WAL group-commit durability |
| `storage { wal_segment_size S; }` | 1 | SIZE 1m–1g | `64m` | restart | |
| `storage { compaction_concurrency N; }` | 1 | NUMBER 1–64 | `cores / 4` (≥ 1) | **live** | bounded `spawn_blocking` pool for compaction and index build |
| `memory { size S; }` | 1 | SIZE | value of buffer `types.memory` | **live** | convenience alias that sets `buffers { types.memory }` |
| `memory { labels { K V; } }` | block | key/value | — | restart | memory-tier labels; semantics owned by `04` |
| `types { block_size S; }` | 1 | SIZE 4k–1m | `64k` | restart | block assembly target ([storage-layout.md](storage-layout.md)) |
| `types { default_compression C; }` | 1 | codec name | `zstd` | **live** (new containers) | persistent/hybrid default; memory mode always defaults to `none` |
| `types { default_compression_level N; }` | 1 | NUMBER in codec range | `3` | **live** | |
| `types { dictionary_max_cardinality N; }` | 1 | NUMBER | `65536` | **live** | per-block dictionary fallback threshold |
| `types { max_composition_depth N; }` | 1 | NUMBER 1–16 | `8` | **live** | reported in the catalog |
| `types { default_encryption_scope S; }` | 1 | `drives \| drives_and_memory` | `drives` | **live** (new containers) | Clarification Q5 |
| `keys { keyring_file P; }` | 1 | PATH | — | live (re-read) | interim `KeyAuthority` until `07`; required when any container declares encryption |

Reserved word `storage` moves from `001`'s "owned by other features" list into this feature; `memory` likewise.

## New buffers registered

| Name | Default | Range | Policy | Owner |
|------|---------|-------|--------|-------|
| `types.memory` | 1 GiB | 16 MiB – 1 TiB | Reject | `03` (re-homed from `002`, which held the interim inventory) |
| `storage.memtable` | 256 MiB | 4 MiB – 256 GiB | Wait | `03` |
| `storage.block_cache` | 512 MiB | 0 – 1 TiB | Reject (LRU evict) | `03` |
| `storage.wal` | 64 MiB | 1 MiB – 16 GiB | Wait | `03` |
| `catalog.metadata` | 32 MiB | 4 MiB – 4 GiB | Reject | `03` |

## New validation codes

| Code | Rule |
|---|---|
| `storage_data_dir_required` | a persistent/hybrid-capable node must declare `storage { data_dir }` |
| `storage_data_dir_unwritable` | path exists and is writable, or can be created |
| `storage_data_dir_in_use{node}` | lock file held by another process |
| `storage_sync_unknown` | value in vocabulary |
| `storage_wal_segment_out_of_range`, `storage_compaction_concurrency_out_of_range` | |
| `memory_size_out_of_range` | within the `types.memory` buffer range |
| `memory_exceeds_buffers{sum, available}` | warning, never fatal (follows `001`'s `buffers_exceed_memory`) |
| `types_block_size_out_of_range` | |
| `types_unknown_codec{c, known}` | `default_compression` is registered |
| `types_codec_level_out_of_range{c, range}` | |
| `types_unknown_encryption_scope{s}` | |
| `types_composition_depth_out_of_range` | 1–16 |
| `keyring_required` | any container declares encryption ⇒ `keys { keyring_file }` present |
| `keyring_unreadable`, `keyring_permissions` (mode > 0600), `keyring_syntax{line}`, `keyring_duplicate{name}` | interim provider validation |

## Effective configuration additions

`EffectiveConfig` gains `storage { data_dir, sync, wal_segment_size, compaction_concurrency }`, `memory { size, labels }`, `types { block_size, default_compression, default_compression_level, dictionary_max_cardinality, max_composition_depth, default_encryption_scope }`, `keys { keyring_file, keys: <count> }` and `catalog { release, digest, types: <count>, containers: <count> }`. Key material is never included (SC-011).

## Admin and CLI additions

Admin ops (token-protected, `admin` and `admin-http` parity per `001` FR-020): `types`, `type`, `codecs`, `capabilities`, `catalog`, `containers`, `describe`, `create-container`, `alter-container`, `drop-container`.

CLI: `spacestorage types [--level L] [--output json]`, `spacestorage type <name>`, `spacestorage codecs`, `spacestorage capabilities`, `spacestorage catalog [--cluster]`, `spacestorage containers <namespace>`, `spacestorage describe <ns>.<name>`, `spacestorage create <ns>.<name> <options…>`, `spacestorage alter <ns>.<name> <options…>`, `spacestorage drop <ns>.<name> [--cascade]`. Option syntax is the flat namespace of [container-definition.md](container-definition.md), with `@file` for `schema`.

## Example

See [fixtures/node-typed.conf](fixtures/node-typed.conf) — a node with admin, one protocol entrypoint, `storage`, `memory`, `types`, `keys` and the new buffers, which must pass `spacestorage validate` unchanged.

```nginx
storage {
  data_dir /var/lib/spacestorage;
  sync fdatasync;
  compaction_concurrency 4;
}

memory {
  size 8g;
  labels { tier ram; }
}

types {
  block_size 64k;
  default_compression zstd;
  default_compression_level 3;
  default_encryption_scope drives;
  max_composition_depth 8;
}

keys {
  keyring_file /etc/spacestorage/keyring;   # interim until feature 07
}
```
