# Contract: `admin-http` / `metrics` handler routes (deltas vs `001`)

**Feature**: `008-observability` | Amends [001 admin-http](../../001-runtime-cli-api/contracts/admin-http.md)

Authentication: `Authorization: Bearer` until `14`; then `METRICS_READ` as in [exposition.md](exposition.md). Health routes unchanged (still unauthenticated).

| Method | Path | This feature | First binary |
|--------|------|--------------|--------------|
| GET | `/metrics` | **200** Prometheus text | required |
| GET | `/metrics/namespaces/{name}` | tenant scrape | slice 9; first-binary profile → 404 `ObservabilitySlice9Required` if the route is compiled out, or 404 `metrics_disabled` |

All other `001` routes unchanged.

## `handler metrics;`

Optional entrypoint. Inventory name `metrics`. Speaks only:

- `GET /metrics`
- `GET /metrics/namespaces/{name}` (slice 9)

No `/v1/*`. Same auth as above. `disable metrics;` allowed (`001` undeclared rule: either one entrypoint **or** explicit disable — **not** mandatory unlike `admin` / `admin-http`; omitting both means the handler is simply absent).

`001` reserved word `metrics` on the config grammar is owned here for the `metrics { }` block and the handler name.
