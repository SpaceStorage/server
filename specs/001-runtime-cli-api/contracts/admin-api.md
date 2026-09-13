# Contract: Administrative API (shared by `admin` and `admin-http`)

**Feature**: `001-runtime-cli-api` | Types in `crates/admin-proto` | Executed by `crates/node/src/admin/`

Both admin handlers expose the same six operations with identical JSON result bodies (FR-020). Transport-specific envelopes are in [admin-tcp-protocol.md](admin-tcp-protocol.md) and [admin-http.md](admin-http.md).

## Authentication (interim, FR-026)

Bearer token, compared in constant time with the trimmed content of `admin.token_file`. Failure → `unauthorized` and, on TCP, connection close. Health probes are exempt and reveal only `state`.

## Operations

### `status`

Result:

```json
{
  "node_name": "db-1",
  "state": "ready",
  "uptime_seconds": 1234,
  "version": "0.1.0",
  "threads": { "total": 8, "busy": 2, "source": "available_cores" },
  "entrypoints": [
    { "name": "admin", "address": "127.0.0.1", "port": 7700, "handler": "admin", "transport": "plaintext", "cert_expired": false, "connections_active": 1 }
  ],
  "buffers": [ { "name": "net.recv", "capacity_bytes": 134217728, "used_bytes": 1048576, "usage_ratio": 0.0078, "limit_hits_total": 0, "policy": "wait", "range": { "min": 1048576, "max": 68719476736 }, "default_bytes": 67108864, "owner": "01-runtime-cli-api" } ],
  "drain_timed_out": false
}
```

### `config`

Result: `EffectiveConfig` as defined in [data-model.md §2](../data-model.md#effectiveconfig-fr-010-fr-012-fr-014-fr-029). Includes `settings[]` with `reload_class` and `pending_restart`, `handlers[]` inventory, and `entrypoints[]`.

### `threads`

Result: `{ "total": 8, "busy": 2, "source": "configured" | "available_cores", "available_cores": 8 }`.

### `buffers`

Result: `{ "buffers": [BufferReport...], "sum_capacity_bytes": n, "memory_available_bytes": n, "exceeds_memory": false }`.

### `reload`

Args: none (re-reads `config_path`). Result: `ReloadReport`:

```json
{
  "ok": true,
  "changed": [ { "setting": "buffers.net.recv", "from": 134217728, "to": 268435456, "class": "live" },
               { "setting": "runtime.threads", "from": 8, "to": 16, "class": "restart_required" } ],
  "applied_live": ["buffers.net.recv"],
  "pending_restart": ["runtime.threads"],
  "warnings": [],
  "errors": []
}
```

Failure modes: `config_invalid` with `errors[]` (nothing changed); `invalid_state` when node is not `ready`.

### `stop`

Args: `{ "wait": bool }` (default false). Result: `{ "accepted": true, "state": "draining", "drain_timeout_secs": 30 }`. With `wait: true` the response is delayed until the node has exited or the timeout elapsed; the transport connection is then closed by process exit — clients treat EOF after `accepted` as success.

## Error body

```json
{ "code": "unauthorized" | "invalid_state" | "config_invalid" | "unknown_op" | "protocol_version" | "internal",
  "message": "human readable",
  "details": { } }
```

`invalid_state.details = { "state": "draining" }`; `config_invalid.details = { "errors": [ConfigError...] }`.

## Parity rule (tested by `crates/node/tests/admin_parity.rs`)

For every operation, `serde_json::Value` of the result obtained over `admin` MUST equal the one obtained over `admin-http` within the same reporting interval, ignoring `uptime_seconds`, `connections_active`, and `threads.busy`.
