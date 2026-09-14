# Contract: Configuration Directives Added by This Feature

**Feature**: `004-distribution-placement` | Extends [`001` grammar](../../001-runtime-cli-api/contracts/config-grammar.md) and [`003` storage/memory](../../003-type-system/contracts/config-directives.md) | Crate: `crates/config`

Reserved words claimed from `001`: `labels` (as a nested block), `cluster`, `replication`. `storage` and `memory` stay owned by `003` and gain `drive` / placement use of `memory.labels`.

## Directives

| Path | Arity | Type | Default | Reload | Notes |
|------|-------|------|---------|--------|-------|
| `node { labels { K V; } }` | block | IDENT/STRING | — | **live** | Open-ended keys |
| `storage { drive NAME { path P; media M; size S?; labels{} } }` | repeatable | PATH, IDENT, SIZE | — | **live** add; path/media restart | `003` `data_dir` still required for persistent |
| `memory { size S; labels { } }` | | SIZE, map | size from `003` | size live; labels live | Absence = no pool |
| `cluster { locality_key K; }` | 1 | IDENT | `region` | **live** | |
| `cluster { token_file P; }` | 1 | PATH | — | live re-read | Required if internodes enabled |
| `cluster { heartbeat_interval D; }` | 1 | DURATION | `2s` | **live** | |
| `cluster { failure_timeout D; }` | 1 | DURATION | `15s` | **live** | |
| `cluster { peers { name N; address A; port P; } }` | repeatable | | — | restart | Self may be omitted |
| `replication { hinted_handoff_window D; }` | 1 | DURATION | `3h` | **live** | Per-group override in container decl |
| `replication { repair_interval D; }` | 1 | DURATION | `24h` | **live** | |
| `replication { repair_bytes_per_sec S; }` | 1 | SIZE | `32m` | **live** | |
| `replication { rebalance_bytes_per_sec S; }` | 1 | SIZE | `64m` | **live** | |
| `replication { async_lag_threshold D; }` | 1 | DURATION | `60s` | **live** | Writes threshold 10000 compiled default, overridable later |
| `replication { max_stamp_skew D; }` | 1 | DURATION | `500ms` | **live** | |
| `replication { txn_timeout D; }` | 1 | DURATION | `30s` | **live** | |
| `disable internode;` | 1 | literal | — | restart | Same enable/disable rule as admin |
| `internode { delay { to NAME D; } }` | test-only | | — | live | `unknown_directive` unless `SPACESTORAGE_TEST=1` |

Handler inventory gains `internode`. Entrypoint example: `entrypoint internodes { address 0.0.0.0; port 7702; handler internode; }`.

## Buffers

| Name | Default | Range | Policy | Owner |
|------|---------|-------|--------|-------|
| `internode.recv` | 64 MiB | 1 MiB–16 GiB | Wait | `04` |
| `internode.send` | 64 MiB | 1 MiB–16 GiB | Wait | `04` |
| `placement.hints` | 256 MiB | 16 MiB–64 GiB | Reject | `04` |
| `placement.repair` | 64 MiB | 8 MiB–16 GiB | Wait | `04` |

## Validation codes

| Code | Rule |
|------|------|
| `internode_handler_undeclared` | internodes enabled or `disable internode;` |
| `cluster_token_required` | token_file present when internodes enabled |
| `cluster_token_unreadable` / `cluster_token_permissions` | exists, readable, mode ≤ 0600 |
| `peer_duplicate_name` / `peer_duplicate_address` | |
| `peer_missing_port` | |
| `drive_media_required` / `drive_path_unreadable` / `drive_name_duplicate` | |
| `memory_size_required` / `memory_exceeds_machine` | |
| `label_duplicate_key` | |
| `locality_key_empty` | |
| `duration_out_of_range` | heartbeat ≥ 100ms, failure_timeout > heartbeat |

RF>1 at runtime without internodes: `InternodesRequired` (not a config-validate error if no such container exists yet).

## Effective configuration

Adds `node.labels`, `storage.drives[]`, `memory.labels`, `cluster{...}` (token path, never token bytes), `replication{...}`, internodes entrypoint, new buffers. Source tags: `configured | built_in | override` (FR-083).

## Admin and CLI

| Op / command | Result |
|--------------|--------|
| `topology` | TopologyView |
| `placement <ns>.<name>` | PlacementReport |
| `replicas <ns>.<name>` | ReplicaSet |
| `health` | degraded placements |
| `rebalance [pause\|resume\|rate]` | plan status |
| `repair` | jobs |
| `txns` | 2PC list |
| `decommission <node>` | plan or blocked |
| `executions` | existing `002`, plus coordinator/acks |

CLI verbs mirror these: `spacestorage topology`, `placement`, `replicas`, `health`, `rebalance`, `repair`, `txns`, `decommission`.
