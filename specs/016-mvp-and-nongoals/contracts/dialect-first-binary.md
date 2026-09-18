# Contract: First-binary protocol dialects

**Feature**: `016-mvp-and-nongoals` | Handlers: `002` | Ceiling: `015` | Spec: FR-005

`015` is the complete-product matrix. This file is the **narrower** first-binary subset. Slice 6 MUST implement the `015` MUST column, not this file.

## PostgreSQL (`postgresql`)

| Item | First binary | Complete product (`015`) |
|------|--------------|--------------------------|
| Frontend/backend 3.0 | MUST | MUST |
| SCRAM-SHA-256 | MUST | MUST |
| Simple query | MUST | MUST |
| Extended query / prepared statements | MUST | MUST |
| INSERT, SELECT, UPDATE, DELETE | MUST | MUST |
| CREATE TABLE / DROP TABLE mapped to Relational Table (or documented mapping) | MUST | MUST |
| Auto-commit | MUST | MUST |
| BEGIN, COMMIT, ROLLBACK | **not-supported** (`0A000`) | MUST |
| COPY | **not-supported** (`0A000`) | MUST |
| Database name as namespace | MUST | MUST |
| PL/pgSQL, LISTEN/NOTIFY, FDW, extensions | not-supported | not-supported |

First-binary SQL is auto-commit only. `BEGIN`, `COMMIT`, and `ROLLBACK` all return `0A000` before opening or acknowledging a transaction. Empty-transaction notices are not the first-binary dialect.

## Redis (`redis`)

On a container of type `K/V Store`:

| Command | First binary |
|---------|--------------|
| AUTH | MUST |
| PING | MUST |
| GET, SET, DEL, EXISTS, SCAN | MUST |
| SELECT | no-op inside the bound namespace |
| TTL / EXPIRE / PTTL mapped to container TTL | MUST |

Off `K/V Store`: canonical JSON/blob only (`002`), including first-binary `Document Store`. Type-specific Redis verbs (`HGET`, `XADD`, `JSON.GET`, …) MUST return a Redis error (unknown command or not-supported) and MUST NOT succeed or no-op (SC-006). Redis Cluster slots, modules, Lua, Redis Streams-as-product: not-supported (`015`).

## Other client protocols

Not in the first-binary build. Config:

```text
entrypoint {
  port 9042;
  handler cassandra;
  plaintext;
}
```

→ validate/start fails `entrypoint_unknown_handler{handler=cassandra, known=[admin, admin-http, internode, postgresql, redis, replication]}`.
