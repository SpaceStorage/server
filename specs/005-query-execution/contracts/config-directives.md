# Contract: `query { }` configuration, admin, CLI

**Feature**: `005-query-execution` | Spec: FR-008, FR-009, FR-021, FR-022 | Crate: `crates/config` + `admin-proto` + `spacestorage`

## Directive

```text
query {
    max_concurrent_per_node 512;
    max_concurrent_per_namespace 128;
    max_memory 256MiB;
    spill off;                    # on | off
    default_concurrency 1;        # 1 = sequential
}
```

All fields optional; missing → defaults above. Live-reload: new queries only. Validation:

| Error code | When |
|------------|------|
| `query_max_concurrent_zero` | node or namespace cap is 0 |
| `query_max_memory_zero` | max_memory is 0 |
| `query_concurrency_zero` | default_concurrency is 0 |
| `query_spill_unknown` | not `on`/`off` |

Effective config reports provenance `configured | built_in`.

## Admin / CLI (FR-009)

| Surface | Op |
|---------|----|
| `GET /v1/executions` | list (existing `002`, extra fields) |
| `GET /v1/executions/{id}` | one record including stage, ranking, acks |
| `POST /v1/explain` | body: SQL or IR JSON; no execute |
| `GET /v1/jobs` / `{id}` / `{id}/watch` | subscribe poll/watch (same job as SQL/Redis/HTTP, slice 8) |
| `POST /v1/jobs/{id}/cancel` | cancel |
| CLI `spacestorage executions`, `explain`, `jobs` | same |

Token-protected like all admin ops (`001`).
