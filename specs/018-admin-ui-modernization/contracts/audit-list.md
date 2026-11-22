# Contract: `audit_list`

**Feature**: `018-admin-ui-modernization` | Record: `014` `AuditEntry` | Op added on `crates/admin-proto`

Read-only. No append, delete, export, or retention change is part of this operation.

## Admin operation

`POST /v1/admin` body:

```json
{
  "op": "audit_list",
  "limit": 50,
  "cursor": null,
  "namespace": null
}
```

| Field | Rule |
|-------|------|
| `limit` | Default 50, max 100. Outside that range → `400` `limit_exceeded` |
| `cursor` | Opaque. Null or omitted → first page. A cursor the server does not recognize → `400` `invalid_cursor` and an empty `entries` array |
| `namespace` | Optional filter. `CLUSTER_ADMIN` may pass null (all entries) or a name. A namespace-scoped principal with `AUDIT_READ` may pass only their namespace; null means "my namespace", and any other name is `403` `forbidden` |

Authorization, before the log is read:

- `CLUSTER_ADMIN` → allow
- `AUDIT_READ` → allow, rows restricted to the principal's namespace ids
- otherwise → `403` `forbidden`, no entries

Success **200**:

```json
{
  "generated_at": "2026-11-22T18:47:00Z",
  "limit": 50,
  "next_cursor": "entry-id-or-null",
  "entries": [
    {
      "id": "018f2c6e-7b1a-7c3d-8e4f-1a2b3c4d5e6f",
      "principal": "admin",
      "action": "namespace.rename",
      "target": "old → new",
      "time": "2026-11-22T18:46:00Z",
      "namespace": "acme"
    }
  ]
}
```

Order: `id` descending. `principal` is `login_at_event`, else the principal id, else `""` for an unknown login. `time` is RFC3339 when the stored HLC can be projected; if it cannot, `time` is null and the shell shows "time unavailable". The response never contains key bytes, token hex, or password material.

`next_cursor` is the last returned `id` when another page exists, otherwise null. The following request passes that value as `cursor` and receives strictly older ids.

The byte size of `entries` stays within the 256 KiB page budget. The implementation stops the page early and sets `next_cursor` rather than exceeding it.

An empty log is **200** with `entries: []` and `next_cursor: null`. The shell's empty state says there are no audit entries. It does not offer a control to change retention.

## Shell route

`GET /v1/ui/audit?limit=&cursor=` calls this operation with the session's scope and returns the same JSON. It is the only audit URL the shell fetches.

## CLI

`POST /v1/admin` with the admin-token bearer may call `audit_list` without CSRF, same as other admin ops. A future `spacestorage audit` table can use this page shape. This feature does not require a new CLI flag beyond the op existing.

## Fixture

[fixtures/audit-page.json](fixtures/audit-page.json) is a success body. Tests assert the keys above are present and that `token`, `password`, `retention`, and `export` are absent.
