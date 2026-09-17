# Contract: Config directives

**Feature**: `010-migration-transforms` | Crate: `config` (`001` grammar registry)

## Block

```nginx
jobs {
    enabled on;              # default off on first-binary profile; on when slice 10 compiled
    catchup_concurrency 4;   # max parallel catch-up scans per node
}
```

Unknown keys → config error (`001`).

## First binary

If `release-profile` is first-binary and `jobs.enabled on` → startup error `MigrateSlice10Required`.

If slice 10 is compiled, default `enabled on`.

## Not in this block

Snapshot retention, WAL, fsync — `013`. Placement — `004`. Quota units — `007`.
