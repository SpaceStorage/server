# Contract: Query option additions (`partial`, `async`)

**Feature**: `005-query-execution` | Spec: FR-011, FR-014 | Extends `002` `query-options.md` | Clarify 2026-09-16 A/B

Two flags join the existing `Sourced` option family (query → session → protocol-native → global/built-in). They do **not** change quorum or isolation.

| Option | Type | Built-in default | Meaning |
|--------|------|------------------|---------|
| `partial` | bool | **off** | If a required shard/part is unreachable, return reachable data plus named unavailability (`Done.partial = true`). Off: fail the whole query. |
| `async` | bool | **off** | Start as a background job; return `exec_id` immediately ([subscribe.md](subscribe.md)). |

## Syntax (same channels as timeout/quorum)

| Protocol | Per query | Per session | Native field |
|----------|-----------|-------------|--------------|
| PostgreSQL / ClickHouse | hint `/*+ spacestorage: partial=on async=on */` | `SET spacestorage.partial = on`; `SET spacestorage.async = on` | none |
| Cassandra | custom payload `spacestorage.partial` / `spacestorage.async` | session payload | none (consistency is **not** this flag) |
| Redis | `SS.WITH partial on …` | `SS.OPTIONS SET partial on` | none |
| Elasticsearch, S3, WebDAV, clickhouse-http | header `X-SpaceStorage-Partial: on`; `X-SpaceStorage-Async: on` | `X-SpaceStorage-Session: partial=on;async=on` | none |

Inspection: existing `SHOW spacestorage.options` / `SS.OPTIONS GET` / applied headers MUST include `partial` and `async` and their source. Execution record stores `applied.partial_ok` and `applied.async_job`.

Unknown token → protocol error; previous session values kept. `partial=on` on a write does not allow reporting a write as successful when write quorum was not met (FR-010 write path unchanged).
