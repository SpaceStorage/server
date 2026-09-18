# Contract: Configuration directives added by this feature

**Feature**: `012-internode-and-time` | Crate: `config` | Extends [`001` grammar](../../001-runtime-cli-api/contracts/config-grammar.md) and [`004` cluster](../../004-distribution-placement/contracts/config-directives.md)

## Entrypoints

| Path | Default | Reload | Notes |
|------|---------|--------|-------|
| `entrypoint { handler internode; }` | required | restart | Address default `127.0.0.1`; starter port 7000 |
| `entrypoint { handler replication; }` | required | restart | Address default `127.0.0.1`; starter port 7001 |
| `tls { }` or `plaintext;` | required | restart | Omitted → `transport_required` |
| `disable internode;` / `disable replication;` | illegal | — | `internode_cannot_disable` |

Handler inventory: `internode`, `replication` (already reserved in `001`).

## Cluster knobs this feature owns

| Path | Default | Reload | Notes |
|------|---------|--------|-------|
| `cluster { heartbeat_interval D; }` | `2s` | **live** | Was `004`; FD uses internodes |
| `cluster { failure_timeout D; }` | `15s` | **live** | **Cluster-wide only**; `004` per-group FD override removed |
| `replication { max_stamp_skew D; }` | `500ms` | **live** | Skew → `node_state=degraded` |

`cluster.locality_key` is **ignored** for voting/HLC (obsolete). Topology ladder stays `004`. `cluster.token_file` stays `011` join secret.

Join config (`011`) MUST include `cluster { quorum_domain NAME; }` on `join` (bootstrap implies `default`).

## Buffers

| Name | Default | Owner |
|------|---------|-------|
| `internode.recv` / `internode.send` | 64 MiB | this feature (was `004`) |
| `replication.recv` / `replication.send` | 64 MiB | this feature |

## Validation codes

| Code | Rule |
|------|------|
| `internode_required` / `replication_required` | both entrypoints present |
| `internode_cannot_disable` | disable refused |
| `cluster_address_required` | join remote while listening on loopback |
| `transport_required` | tls or plaintext |
| `quorum_domain_required` / `quorum_domain_unknown` | join |
