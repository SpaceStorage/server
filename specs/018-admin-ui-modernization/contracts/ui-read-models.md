# Contract: Shell read models

**Feature**: `018-admin-ui-modernization` | Composer: `crates/admin-ui` | Served by `admin-http`

All routes require a valid `ss_session` cookie (or the admin-token bearer for non-browser callers). They are GET, so they do not require CSRF. They do not accept a session hex as `Authorization: Bearer`.

Every success body includes `generated_at` (RFC3339, `Z`). List routes use [research.md](../research.md) R5: `limit` default 50, max 100, else `400` `limit_exceeded`; `cursor` optional; `next_cursor` null at the end; JSON ≤ 256 KiB.

A composer that cannot finish within **1.5 s** returns **504**:

```json
{ "code": "deadline_exceeded", "message": "ui read exceeded budget", "details": {} }
```

Where a snapshot has several panels, a failed dependency sets that panel's `status` to `unavailable` and still returns **200** for the rest. See [data-model.md](../data-model.md).

Tenant filter is server-side. A namespace admin receives only their namespaces, the nodes that host their containers or shards, and their series. Another tenant's name is not in the body. A direct GET for another namespace is **403** `forbidden` with `details` empty.

## GET /v1/ui/overview

Cluster Overview Snapshot. Degraded members and lagging replicas are in the body (capped at 20 named items) so the shell can place them above the fold. The body is not a `ClusterMapView`.

Fixture: [fixtures/overview-degraded.json](fixtures/overview-degraded.json).

`source` is `controller` when membership and the control plane answered, and `node-local` when the process has no roster and is reporting itself. A missing controller does not mark members `ready`.

## GET /v1/ui/namespaces

Query: `limit`, `cursor`, `q` (name substring, case-sensitive, applied before paging).

```json
{
  "generated_at": "2026-11-22T18:47:00Z",
  "limit": 50,
  "next_cursor": null,
  "namespaces": [
    {
      "name": "acme",
      "container_count": 2,
      "container_count_complete": true,
      "quota": { "bytes": 1073741824 },
      "health": "ok"
    }
  ]
}
```

Empty cluster, cluster admin: **200** with `namespaces: []`. The shell's empty state offers create. That empty array is not an error.

`container_count_complete: false` means the 10 000-descriptor cap stopped the scan. The UI says "at least {container_count}".

Fixture: [fixtures/namespace-page.json](fixtures/namespace-page.json).

## GET /v1/ui/namespaces/{name}

**200** namespace view plus paged containers (`limit`, `cursor` on this route too):

```json
{
  "generated_at": "2026-11-22T18:47:00Z",
  "name": "acme",
  "container_count": 2,
  "container_count_complete": true,
  "quota": { "bytes": 1073741824 },
  "health": "ok",
  "record": {},
  "containers": {
    "limit": 50,
    "next_cursor": null,
    "items": [
      {
        "name": "orders",
        "model": "kv_store",
        "mode": "persistent",
        "shards": 4,
        "replicas_degraded": 0
      }
    ]
  }
}
```

`record` is the `007` describe object, passed through. `shards` or `replicas_degraded` is JSON `null` when the map composer has no entry; the UI prints "unavailable". The page does not include container rows.

Unknown name, or a name outside the principal's namespaces: **403** `forbidden`.

## GET /v1/ui/nodes

Query: `limit`, `cursor`, `q`, `status` (`ready`, `not_ready`, `pending`, `unknown`).

```json
{
  "generated_at": "2026-11-22T18:47:00Z",
  "source": "controller",
  "limit": 50,
  "next_cursor": null,
  "nodes": [
    {
      "node_id": "018f2c6e-7b1a-7c3d-8e4f-1a2b3c4d5e6f",
      "node_name": "n2",
      "readiness": "not_ready",
      "controller_role": "voter"
    }
  ]
}
```

Pending joins use `readiness: "pending"`. A member whose reporter did not answer is `unknown`.

## GET /v1/ui/nodes/{id}

**200** node detail. Fields the local admin surface does not have for a peer are JSON `null`.

```json
{
  "generated_at": "2026-11-22T18:47:00Z",
  "node_id": "018f2c6e-7b1a-7c3d-8e4f-1a2b3c4d5e6f",
  "node_name": "n1",
  "readiness": "ready",
  "controller_role": "leader",
  "source": "controller",
  "state": "ready",
  "worker_threads": 8,
  "buffers": [{ "name": "request.queue", "used": 0, "capacity": 1024 }],
  "labels": { "rack": "r1", "region": "eu" },
  "reported_by": "018f2c6e-7b1a-7c3d-8e4f-1a2b3c4d5e6f"
}
```

A namespace admin requesting a node that hosts none of their containers or shards: **403** `forbidden`.

## GET /v1/ui/monitoring

Query: `namespace` optional.

- Cluster admin or cluster `METRICS_READ`, no `namespace`: cluster scope, series from the node registry.
- With `namespace`: that namespace's series, same names and labels as `008`.
- Namespace admin requesting cluster scope, or another namespace: **403** `forbidden`.

```json
{
  "generated_at": "2026-11-22T18:47:00Z",
  "scope": "namespace",
  "namespace": "acme",
  "series": [
    {
      "name": "spacestorage_api_requests_total",
      "labels": { "namespace": "acme", "application": "spacestorage-ui", "protocol": "admin-http" },
      "value": 12
    }
  ],
  "unavailable": [
    { "name": "spacestorage_replication_lag_seconds", "reason": "series absent" }
  ]
}
```

The example series name is illustrative. The implementation emits whatever families `008` actually registered for that scope, under those exact names. An absent family is listed in `unavailable` and is not emitted as `0`.

## GET /v1/ui/jobs

Query: `limit`, `cursor`.

```json
{
  "generated_at": "2026-11-22T18:47:00Z",
  "limit": 50,
  "next_cursor": null,
  "jobs": [
    {
      "id": "job-1",
      "kind": "migration",
      "target": "acme/orders",
      "status": "running",
      "progress": { "bytes_copied": 10, "bytes_remaining": 90 },
      "failure": null
    }
  ]
}
```

## GET /v1/ui/audit

Proxies [audit-list.md](audit-list.md). Same page contract. **403** when the principal has neither `AUDIT_READ` nor `CLUSTER_ADMIN`. The shell renders no retention or export control; this route accepts no `delete` or `config` query.

## Writes the shell calls (owning routes)

Not composed here. After the confirmation in [data-model.md](../data-model.md), `shell.js` calls the existing route with the cookie, `Origin`, and `X-SpaceStorage-CSRF`:

| Action | Request |
|--------|---------|
| Namespace create, rename, delete, quota replace | `POST /v1/admin` with the existing `AdminOp` |
| Reload | `POST /v1/reload` |
| Cluster stop | `POST /v1/stop` |
| Drain, undrain | `POST /v1/admin` with [drain-admin.md](drain-admin.md) |
| Job cancel, resume | `POST /v1/jobs/{id}/cancel` or `/resume` |

The shell updates its tables only from a following GET. The write response is the owning surface's body and is shown as the outcome message.
