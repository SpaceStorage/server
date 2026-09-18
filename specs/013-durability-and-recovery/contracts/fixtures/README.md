# Fixtures

Configs for [quickstart.md](../../quickstart.md). Combine with `012` internodes/admin entrypoints when running a full node; durability tests may start storage in-process without fabric.

| File | Purpose |
|------|---------|
| [single-drive.conf](single-drive.conf) | Implicit `default` drive = `data_dir`; first-binary WAL |
| [two-drive.conf](two-drive.conf) | Two WALs; disk-full isolation (SC-003) |
| [invalid/sync-none.conf](invalid/sync-none.conf) | `sync none` → startup fail |
