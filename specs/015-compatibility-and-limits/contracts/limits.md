# Contract: Size, connection, and admission limits

**Feature**: `015-compatibility-and-limits` | Spec: FR-010–FR-012 | See also `005` [admission.md](../../005-query-execution/contracts/admission.md)

## Size and connections (`limits { }`)

| Knob | Default | When checked |
|------|---------|----------------|
| `max_key` | 1KiB | key / path / object key parse |
| `max_value` | 16MiB | value / document / assembled S3 object (multipart sum) |
| `max_query_text` | 1MiB | statement / CQL / ES body / CH query |
| `max_result` | 64MiB | while producing rows/payload |
| `max_connections_per_entrypoint` | 10000 | accept |
| `max_connections_per_principal` | 1000 | after AUTH (`014` principal id) |

Over → `limit_exceeded{limit,current,max}` in the protocol form. Never hang. Never truncate a result silently.

## Query admission (`query { }`, owned by `005`, policy owned here)

| Knob | Default | Policy |
|------|---------|--------|
| `max_concurrent_per_node` | 512 | extra query **rejects** |
| `max_concurrent_per_namespace` | 128 | extra query **rejects**; `007` may lower |
| `max_memory` | 256MiB | FirstBinary/HandlersComplete: **reject**. CompleteProduct: **spill** if the plan marks sort/hash/agg spillable; else **reject** |
| `spill` | `off` in FB/HC; `on` in CP | operator MAY set `off` on CP → all over-memory rejects |

Spill MUST NOT satisfy a concurrent-query miss. CPU hard isolation MUST NOT be claimed. Quota **units** (bytes stored, counts, connections, optional ops) remain `007`; hitting a quota is `007`’s named error, same hard-reject spirit.

Buffer-full (`001`) → `admission_rejected{limit:"buffer"}`.
