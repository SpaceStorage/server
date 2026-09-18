# Contract: PostgreSQL cursor subset

**Feature**: `015-compatibility-and-limits` | Spec: FR-014 | This **is** the intent’s “documented holdable subset”

## FirstBinary / HandlersComplete

SQL `DECLARE` / `FETCH` / `CLOSE` → not-supported (`copy`/`cursor_not_in_profile`). Extended-query portals (unnamed/named, Execute row count) remain as `002`/`016`.

## CompleteProduct MUST

Inside an **open** transaction:

- `DECLARE name [NO SCROLL] [BINARY] CURSOR [WITHOUT HOLD] FOR query`
- `FETCH [FORWARD] [count|ALL] FROM name`
- `CLOSE name`

Cursor is a portal bound to the transaction id. COMMIT, ROLLBACK, or disconnect drops it.

## CompleteProduct MUST NOT

- `WITH HOLD`
- `SCROLL` / `INSENSITIVE` holdable variants
- `FETCH BACKWARD` / `PRIOR` / absolute except FORWARD
- Using the cursor after COMMIT
- Hold across session

Error: PG `0A000` naming the construct. Never succeed empty.
