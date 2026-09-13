# Contract: Quorum and Timeout Options Across Protocols

**Feature**: `002-protocol-drivers` | Module: `crates/protocol-core/src/options.rs` | Spec: FR-021–FR-031; Clarification Q1

## Vocabulary (identical on every protocol)

`ONE`, `TWO`, `THREE`, `QUORUM`, `LOCAL_QUORUM`, `EACH_QUORUM`, `LOCAL_ONE`, `ALL`, or an integer acknowledgement count `N` (`1`..`65535`, also written `ACKS=N`). Case-insensitive on input; emitted upper-case.

Timeout: duration literal `<int>(ms|s|m|h)` or a bare integer meaning **milliseconds** (Redis, ClickHouse settings) or **seconds** where the protocol's native field is in seconds (Elasticsearch `timeout=5s` style strings are parsed as literals). Must be > 0.

## Precedence and resolution

```text
quorum  = query ?? session ?? namespace_default (07, later) ?? global.(write_quorum | read_quorum)
timeout = min(query ?? session ?? namespace_default ?? global.timeout, global.max_timeout)
```

`write_quorum` vs `read_quorum` is selected by `LogicalRequest::is_mutation()`. Clamping:

| Condition | Explicit (query/session) | Default-sourced |
|---|---|---|
| quorum unsatisfiable by replica count | reject `QuorumUnsatisfiable{requested, available}` before execution | clamp to highest satisfiable; `clamped = ReplicaCount` |
| timeout > `max_timeout` | clamp; `clamped = MaxTimeout` | clamp; `clamped = MaxTimeout` |

## Global defaults (configuration)

```nginx
query_defaults {
  write_quorum TWO;     # default TWO
  read_quorum  ONE;     # default ONE
  timeout      30s;     # default 30s
  max_timeout  10m;     # default 10m; optional
}
```

Live-reloadable; each query snapshots the defaults at start (FR-027). Effective configuration reports `source: configured | built_in`.

## Per-protocol syntax

| Protocol | Session-level | Per-query | Native fields honoured | Inspection |
|---|---|---|---|---|
| **postgresql** | `SET spacestorage.quorum = 'QUORUM'; SET spacestorage.timeout = '5s';` (also `SET LOCAL` inside a transaction; `statement_timeout` maps to timeout) | leading hint comment `/*+ spacestorage: quorum=TWO timeout=5s */ SELECT …` | `statement_timeout` (session) | `SHOW spacestorage.quorum`, `SHOW spacestorage.timeout`, `SELECT * FROM spacestorage.options` → rows `(option, value, source, clamped_from)` for the last executed statement and current session |
| **cassandra** | custom payload on any frame: `spacestorage.quorum`, `spacestorage.timeout` (UTF-8 values) | consistency flag in `QUERY`/`EXECUTE`/`BATCH` (native); `USING TIMEOUT 5s` clause (native, Scylla-compatible); custom payload on that frame | consistency level; `USING TIMEOUT` | `SELECT * FROM system.spacestorage_options` |
| **redis** | `SS.OPTIONS SET quorum TWO timeout 5000` | `SS.WITH quorum TWO timeout 5000 GET k` (wraps any command) | — | `SS.OPTIONS GET` → array of `option value source clamped_from`; `SS.LAST` → options applied to the previous command |
| **elasticsearch** | header `X-SpaceStorage-Session: quorum=TWO;timeout=5s` (applies to subsequent requests on the same connection) | headers `X-SpaceStorage-Quorum`, `X-SpaceStorage-Timeout`; query params `spacestorage_quorum`, `spacestorage_timeout` | `timeout=` (search/index), `wait_for_active_shards=N` (→ write quorum `N`; `all` → `ALL`) | response headers `X-SpaceStorage-Applied-Quorum`, `X-SpaceStorage-Applied-Timeout`, `X-SpaceStorage-Options-Source: quorum=session;timeout=global,clamped=max_timeout`; `GET /_spacestorage/options` |
| **clickhouse** (native) | `SET spacestorage_quorum = 'TWO'; SET spacestorage_timeout = 5000;` (settings persist on the connection) | `SELECT … SETTINGS spacestorage_quorum='TWO', spacestorage_timeout=5000` | `max_execution_time` (seconds) → timeout | `SELECT * FROM system.spacestorage_options` |
| **clickhouse-http** | `?session_id=…` with `SET …`, or headers as Elasticsearch | `SETTINGS` clause or query params `spacestorage_quorum=`/`spacestorage_timeout=`; headers `X-SpaceStorage-*` | `max_execution_time` param | same response headers as Elasticsearch; `system.spacestorage_options` |
| **s3** | header `X-SpaceStorage-Session` (per connection) | headers `X-SpaceStorage-Quorum`, `X-SpaceStorage-Timeout` (may be excluded from `SignedHeaders`) | — | response headers `X-SpaceStorage-Applied-*`, `X-SpaceStorage-Options-Source` |
| **webdav** | header `X-SpaceStorage-Session` | headers `X-SpaceStorage-Quorum`, `X-SpaceStorage-Timeout` | `Timeout:` header on `LOCK` applies to the lock only, not the query | same response headers |

Rules:

- Invalid values → protocol's `InvalidOptionValue` error; session unchanged (FR-031).
- A session-level change never affects the in-flight request (spec edge case).
- Per-query values apply to that request only.
- Every response to a request that executed carries the applied values in the protocol's inspection form (HTTP: always in headers; SQL/CQL/RESP: on demand via the inspection query/command, plus in the text of timeout/quorum errors).

## Session-inspection row shape (all protocols)

| option | value | source | clamped_from |
|---|---|---|---|
| `quorum` | `ONE` | `global` | `TWO (replica_count=1)` |
| `timeout` | `30s` | `global` | — |

`source ∈ query | session | namespace | global`.

## Execution record (FR-037)

`GET /v1/executions` (admin) returns the same applied values per executed request; the conformance suite uses it to assert SC-004.
