# Contract: Promote and epoch fence

**Feature**: `012-internode-and-time` | Crates: `replication`, `controlplane`, `admin-proto` | Spec: FR-010

## Admin

```text
spacestorage promote <container> --to <follower-domain>
spacestorage promote <container> --to <follower-domain> --force --accept-data-loss
```

`CLUSTER_ADMIN` only. Manual. No auto-promote on FD timeout.

## Ordinary promote

Allowed when **either**:

1. Follower has applied through the last **known** source-log position at the current epoch, **or**
2. Every member of the current source domain is failure-detector **unavailable**.

Otherwise `PromoteNotCaughtUp`.

## Force promote

Requires `--force` **and** `--accept-data-loss`. Without the accept → `PromoteNeedsDataLossAccept`. Allowed when A is still heartbeating and B is lagging.

## On success

1. `epoch += 1`.
2. `source_domain = to_domain`.
3. Broadcast `FenceEpoch { container, epoch, old_source }`.
4. Old source MUST NOT accept writes until it rejoins as a **follower** (applies the new source log).
5. Two live sources → protocol-refused (`TwoSources`).

Old-epoch `FanoutWrite` / WAL apply → `EpochFenced`.
