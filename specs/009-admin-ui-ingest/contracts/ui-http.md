# Contract: Admin UI HTTP

**Feature**: `009-admin-ui-ingest` | Router: `crates/admin-ui` mounted on `admin-http` (`001`)

Authentication: same bearer as [admin-http.md](../../001-runtime-cli-api/contracts/admin-http.md). After `014`, permission checks below. First-binary profile: all `/ui/*` and `/v1/cluster/map` → `404` `{ "code": "UiIngestSlice11Required" }` if the feature is compiled out; if compiled but gated, same code.

Application name on `005`/`008`: `spacestorage-ui`.

## Pages (HTML)

| Method | Path | Who | Success |
|--------|------|-----|---------|
| GET | `/ui/cluster` | map rights (below) | 200 HTML Cerebro-like map |
| GET | `/ui/console` | any authenticated principal with at least one namespace or cluster admin | 200 HTML Kibana-like console |
| GET | `/ui` | same as console | 302 `/ui/console` |

Poll interval default **2 s** (query `?interval=` 1–10). Assets under `/ui/assets/*` (checked-in JS/CSS).

## JSON

| Method | Path | Authz | Success | Errors |
|--------|------|-------|---------|--------|
| GET | `/v1/cluster/map` | `CLUSTER_ADMIN` or cluster `METRICS_READ` → full map; `NAMESPACE_ADMIN` → namespace scope; else 403 | 200 [ClusterMapView](../data-model.md) | 401, 403 `forbidden` |
| GET | `/v1/cluster/map?namespace=acme` | must be allowed that namespace | 200 scoped map | 403 if cluster-only caller asks for a namespace they may not see |
| POST | `/v1/console/query` | `READ` on named containers; body is a `005` query | 200 result page (respect `015` size; paginate) | 401, 403, 413, 422 |
| POST | `/v1/console/config` | `CLUSTER_ADMIN` for cluster settings; `NAMESPACE_ADMIN` for that namespace | 200 | 401, 403 `forbidden` on cross-scope |
| POST | `/v1/console/containers` | `CREATE` + `NAMESPACE_ADMIN` (or `CLUSTER_ADMIN`) | 201 | 403 |

`POST /v1/console/query` MUST execute in `005` (FR-004, SC-006). The UI MUST NOT parse SQL/filters locally into storage reads.

## Map rights (clarify Q1)

| Principal | `/ui/cluster` and `/v1/cluster/map` |
|-----------|--------------------------------------|
| `CLUSTER_ADMIN` or cluster-scoped `METRICS_READ` | `scope=cluster` |
| `NAMESPACE_ADMIN` only | `scope=namespace`, 0 foreign containers |
| other | 403, no body |

## Console

Tenant `NAMESPACE_ADMIN` on `acme`: configure `acme`, browse `acme`, **403** on cluster-global settings (US2.2, SC-003). Pagination: follow `015` result limits; never buffer an entire object store in one response (spec edge case).
