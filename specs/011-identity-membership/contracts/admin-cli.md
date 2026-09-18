# Contract: Admin API and CLI

**Feature**: `011-identity-membership` | Crates: `admin-proto`, `spacestorage`, `node` | Extends `001` CLI / [`006` admin](../../006-control-plane/contracts/admin-cli.md)

Parity over `admin` and `admin-http` (`001` FR-020). First binary auth: admin bearer = CLUSTER_ADMIN.

## Admin ops (JSON, both handlers)

| Op | Result / errors |
|----|-----------------|
| `Membership` | `{ cluster_uuid, cluster_name, members[], pending[], retired_count }` |
| `Admit { node_id }` | member view; `not_pending`, `name_in_use`, `Minority` |
| `JoinTokenMint { node_name, node_id?, ttl? }` | `{ token, expires_at }` (token shown once) |
| `Drain { node_id }` / `Undrain { node_id }` | `{ status }`; `not_member`; undrain of non-draining → `invalid_state` |
| `Decommission { node_id, accept_data_loss }` | `{ removed }` or `DecommissionBlocked` |
| `Replace { node_id }` | prepares replace-eligibility check; the new process still sends `JoinRequest`; `live_replace` if heartbeating |
| `SecretRotateBegin` / `SecretRotateComplete` | `{ epoch }` |

`Stop` remains `001` (process exit).

## CLI

```text
spacestorage membership
spacestorage admit <node-id>
spacestorage join-token mint --name <node-name> [--id <uuid>] [--ttl 12h]
spacestorage drain <node-id|name>
spacestorage undrain <node-id|name>
spacestorage decommission <node-id|name> [--accept-data-loss]
spacestorage replace <node-id>
spacestorage cluster secret-rotate begin|complete
```

`--output json` supported. Exit codes: `001` plus 3 = `NotLeader`/`Minority` (retryable), 2 = validation (`ladder`, `retired_identity`, `live_replace`, `DecommissionBlocked`).
