# Contract: Config directives

**Feature**: `006-control-plane` | Crate: `config` | Spec: FR-019, FR-020 | Research R16

## `cluster.raft` block

```nginx
cluster {
    raft {
        heartbeat 500ms;
        election_timeout 2s;
    }
    controller_exclusive_data off;    # default; `on` requires controlplane-ops (slice 7)
}
```

Omitted `raft` → documented defaults. `election_timeout` MUST be > `heartbeat`. `heartbeat 0` / `election_timeout 0` → validation error.

Loopback conformance fixtures: `heartbeat 50ms; election_timeout 300ms;` ([fixtures/raft-block.conf](fixtures/raft-block.conf)).

## Exclusive data

- Omitted or `off`: voters MAY hold tenant replicas (default).
- `on` without `controlplane-ops`: `Slice7Required` / `unknown_directive` at validate (first binary).
- `on` with slice 7: set flag; if tenant replicas remain on voters → `ExclusiveDataBlocked` until drain/rebalance (`04`/`11`).

Reload: timeouts and exclusive-data are **live** (`001` reload class). Changing exclusive-data to `on` does not drop data.
