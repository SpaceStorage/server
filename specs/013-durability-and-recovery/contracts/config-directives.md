# Contract: Config directives

**Feature**: `013-durability-and-recovery` | Crate: `crates/config` | Extends `003`/`004` `storage { }`

```text
storage {
  data_dir "/var/lib/spacestorage";
  sync fdatasync;              # fsync | fdatasync ; none = startup error unless test
  gc_grace 24h;                # namespace/container override allowed
  wal_segment 64MiB;
  group_commit { max_wait 2ms; max_bytes 1MiB; }
  drive nvme0 { path "/data/nvme0"; media nvme; }
  drive hdd0  { path "/data/hdd0";  media hdd; }
}
```

`drive` blocks are `004`. This feature requires a WAL directory on each drive that hosts persistent/hybrid data.

Invalid:

- omitted `sync` → default `fdatasync` (not plaintext-style omit error).
- `sync none` without `SPACESTORAGE_TEST=1` → `sync_none_not_durable`.
- `gc_grace 0` → refused (`gc_grace_too_small`).

Namespace/container: `gc_grace` option on the definition overrides node default (`003` option namespace).
