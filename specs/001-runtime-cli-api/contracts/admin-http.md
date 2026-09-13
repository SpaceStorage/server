# Contract: `admin-http` handler routes

**Feature**: `001-runtime-cli-api` | Router in `crates/node/src/handler/admin_http.rs` (axum)

All bodies are `application/json`. Authentication: `Authorization: Bearer <token>` on every route except `/v1/health/*`. Missing/invalid → `401` with the error body from [admin-api.md](admin-api.md).

| Method | Path | Op | Success | Errors |
|--------|------|----|---------|--------|
| GET | `/v1/status` | `status` | 200 | 401 |
| GET | `/v1/config` | `config` | 200 | 401 |
| GET | `/v1/threads` | `threads` | 200 | 401 |
| GET | `/v1/buffers` | `buffers` | 200 | 401 |
| POST | `/v1/reload` | `reload` | 200 (`ok:true`), 200 with `pending_restart` non-empty | 401; 409 `invalid_state`; 422 `config_invalid` (body = `ReloadReport` with `errors[]`) |
| POST | `/v1/stop` | `stop` | 202 `{accepted:true,...}`; with `?wait=true` the response may be cut by process exit (client treats connection close after 202 headers as success) | 401; 409 `invalid_state` (already draining) |
| GET | `/v1/health/live` | — | 200 `{"state": "<state>"}` in `starting|ready|draining` | — |
| GET | `/v1/health/ready` | — | 200 `{"state":"ready"}` | 503 `{"state": "<state>"}` otherwise |
| GET | `/metrics` | — | reserved for feature `08` | 404 in this feature |
| * | anything else | — | — | 404 `{ "code": "unknown_op" }` |

Rules:

- Only HTTP/1.1 in this feature (HTTP/2 via ALPN may be enabled with TLS in a later feature).
- Request bodies larger than 1 MiB → 413. No request body is required by any route.
- Responses include `X-SpaceStorage-Node: <node_name>` and `X-SpaceStorage-State: <state>`.
- A `GET` to a non-admin path (e.g. `/_cluster/health`) is `404 unknown_op` — this port speaks only `admin-http` (FR-019).
- Keep-alive connections are closed by the server when the node enters `draining`, after the in-flight response completes.
