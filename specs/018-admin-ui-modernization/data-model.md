# Data Model: Modern Admin Web Console

**Feature**: `018-admin-ui-modernization` | **Date**: 2026-11-22

These are presentation records. They are not a second catalog, membership store, or audit log. Field sources are named. Validation is the owning surface's; the shell displays the server's reason.

Page fields shared by every list: `limit` (1–100, default 50), `cursor` (opaque, optional), `next_cursor` (null on the last page), `generated_at` (RFC3339 UTC). See [research.md](research.md) R5.

## UI Session View

Returned by `POST /v1/auth/login` and `GET /v1/auth/session`. The session secret is not a field of this view. It travels only in the `ss_session` cookie ([contracts/session-cookie.md](contracts/session-cookie.md)).

| Field | Type | Rules |
|-------|------|--------|
| `principal` | string | Login recorded by `014` at issue |
| `principal_id` | UUID | `014` principal id |
| `scope` | `cluster` \| `namespace` | `cluster` iff `CLUSTER_ADMIN` |
| `namespaces` | string[] | Empty when `scope` is `cluster`. Otherwise the namespaces this principal may administer. Never another tenant |
| `expires_at` | RFC3339 | Issue time + 12 h. No idle extension |
| `expires_in` | integer seconds | Remaining lifetime, for the login response |
| `csrf_token` | hex string | Bound to the session hash. Not the session secret. Stable until logout or expiry |
| `affordances` | object of booleans | Derived from `014` verbs. See below |

`affordances` keys, each true only when the verb allows the action server-side:

| Key | Verb |
|-----|------|
| `namespace_write` | `CLUSTER_ADMIN` |
| `quota_replace` | `CLUSTER_ADMIN` |
| `config_reload` | `CLUSTER_ADMIN` |
| `cluster_stop` | `CLUSTER_ADMIN` |
| `node_drain` | `CLUSTER_ADMIN` |
| `job_control` | `CLUSTER_ADMIN` or `MIGRATE` on the job's scope, matching `010` |
| `audit_read` | `CLUSTER_ADMIN` or `AUDIT_READ` |
| `metrics_read` | `CLUSTER_ADMIN`, cluster `METRICS_READ`, or namespace `METRICS_READ` for `namespaces` |
| `map` | Same rule as `009` map authz |
| `browse` | Same rule as `009` console authz |

Hiding a control when the flag is false is not the enforcement. The route still checks the verb.

**Transitions**: anonymous → signed-in (login) → signed-out (logout revokes the hash and clears the cookie) or expired (resolve returns none; next call is 401). A stale cookie from another process is the expired path, with `code` `session_invalid`, distinct from login `unauthorized` / "invalid admin token".

## Navigation Section

Not persisted. One row of the shell's route table ([contracts/shell-http.md](contracts/shell-http.md)).

| Field | Type | Rules |
|-------|------|--------|
| `id` | string | `overview`, `namespaces`, `nodes`, `monitoring`, `jobs`, `audit`, `map`, `browse` |
| `path` | string | Under `/ui` |
| `label` | string | SpaceStorage noun. Never index, document, ILM, or data stream |
| `requires` | affordance key | Section omitted from nav when false. Direct load shows a refusal and no data |

## Cluster Overview Snapshot

`GET /v1/ui/overview`. Point in time. Each panel has `status`: `ok` or `unavailable`. An `unavailable` panel carries `reason` and no fabricated zero.

| Panel | Fields when `ok` | Source |
|-------|------------------|--------|
| `readiness` | `value`: `ready` \| `not_ready` \| `unknown` | `001` health / node state |
| `members` | `ready`, `not_ready`, `pending`, `unknown` counts; `degraded[]` of `{node_id, node_name, readiness}` capped at 20, plus `degraded_omitted` | `011` roster. If no roster, one local node and `source: "node-local"` |
| `controller` | `available` bool; `raft` `leader` \| `unknown` | `006`. Unavailable is explicit, never painted as healthy |
| `counts` | `namespaces` (exact, from the in-memory `007` list length); `containers`, `container_count_complete` | Namespace length from the controller. Containers from the bounded descriptor scan ([research.md](research.md) R6) |
| `replicas` | `healthy`, `degraded`, `lagging`; `named[]` of `{namespace, container, replica, lag_ms}` capped at 20, plus `named_omitted` | `009` map composer / `004` placement, summarized server-side. Not the full map document |
| `jobs` | `active`, `failed`, `recently_completed` | `010` |
| `source` | `controller` \| `node-local` | FR-026 |

`generated_at` is the snapshot time. The client marks the overview stale when that timestamp is older than 30 s.

## Namespace View

One element of `GET /v1/ui/namespaces`.

| Field | Type | Rules |
|-------|------|--------|
| `name` | string | `007` name. The shell does not rewrite it |
| `container_count` | integer | Descriptors seen for this name during the capped scan |
| `container_count_complete` | bool | False when the 10 000 cap stopped the scan |
| `quota` | object \| null | Server quota summary from `007`. Null when the namespace has none. The shell does not invent zeros |
| `health` | string | `ok`, `degraded`, or `unknown`. `unknown` when replica summary is unavailable |

**Namespace detail** (`GET /v1/ui/namespaces/{name}`) adds the `007` describe record and a paged `containers` inventory:

| Field | Type | Rules |
|-------|------|--------|
| `name` | string | Container name |
| `model` | string | `L3Model` as stored |
| `mode` | string | `StorageModeChoice` as stored |
| `shards` | integer \| null | From the `009` map entry for this namespace+name. Null → the UI shows "unavailable" |
| `replicas_degraded` | integer \| null | Same. Null when the map has no entry |

No row payload. Authoring and row browse are links to `/ui/browse` and `/ui/map`.

**Writes** (not stored on this view; they return the owning surface's body):

| Action | Call | Confirmation |
|--------|------|----------------|
| Create | `AdminOp::NamespaceCreate` | Name, effect "create namespace" |
| Rename | `AdminOp::NamespaceRename` | Old and new name. Identity stays the `007` id |
| Delete | `AdminOp::NamespaceDelete { cascade }` | Namespace name and container count. Cascade must be explicit |
| Quota replace | `AdminOp::QuotaReplace` | Namespace name and the proposed quota text |

A refused create, rename, delete, or quota replace leaves the previous view in place. The UI shows `message` from the error body.

**Tenant rule**: a namespace-scoped principal's list contains only `session.namespaces`. `GET` of another name is `403` with no `name` echo that would confirm existence beyond the refusal code `forbidden`.

## Node View

One element of `GET /v1/ui/nodes`.

| Field | Type | Rules |
|-------|------|--------|
| `node_id` | UUID string | Membership id, or the local node id when there is no roster |
| `node_name` | string | As reported |
| `readiness` | `ready` \| `not_ready` \| `pending` \| `unknown` | `pending` is a pending join, not a member. Unreachable reporter → `unknown`, never `ready` |
| `controller_role` | string \| null | From `006` when present. Null → "unavailable" |
| `source` | `controller` \| `node-local` | |

**Node detail** adds, each either a reported value or `null` (UI: "unavailable"):

| Field | Source |
|-------|--------|
| `state` | `001` status for that node when this process is that node; otherwise the membership status. Do not invent peer-local thread counts |
| `worker_threads` | `001` threads, only for the local node. Peers: null unless the membership/admin surface already exposes them |
| `buffers` | `001` buffers, same locality rule |
| `labels` | Topology labels already on the member or node config. Keys of the cluster ladder included when present (`rack`, `az`, `region`, `continent`, `planet`, and any disk/memory labels) |
| `reported_by` | Node id that produced `state`, `worker_threads`, and `buffers` |

A namespace admin sees only nodes that appear as replica hosts for their namespace's containers on the `009` map snapshot. No cluster-wide capacity figure is added to that payload. A principal with neither cluster scope nor a namespace sees no Nodes section (`affordances` has no node list; the route returns `403`).

Filter query `q` matches name, `status` matches `readiness`. Both are echoed in the URL by the shell.

## Monitoring Panel

`GET /v1/ui/monitoring` and `GET /v1/ui/monitoring/namespaces/{name}`.

| Field | Type | Rules |
|-------|------|--------|
| `scope` | `cluster` \| `namespace` | Namespace admins cannot request `cluster` (`403`) |
| `namespace` | string \| null | Required when `scope` is `namespace` |
| `series` | array | Each item: `name` (exact `008` family), `labels` (existing label set, unmodified), `value` or `buckets` as the registry holds them |
| `unavailable` | array of `{name, reason}` | Absent series. Never substituted with 0 |
| `generated_at` | RFC3339 | |

The composer reads the in-process `008` registry. It does not scrape itself over HTTP and does not rename families. UI's own traffic is visible on the existing `spacestorage_api_requests_total`, `spacestorage_api_request_errors_total`, and `spacestorage_api_request_duration_seconds` with `application="spacestorage-ui"` and `protocol="admin-http"`. Those label names already exist; this feature adds no label key.

## Job Progress View

`GET /v1/ui/jobs`, element of `jobs[]`. Source: `010` `GET /v1/jobs` and `GET /v1/jobs/{id}`, paged by this feature when the underlying list is longer than `limit`.

| Field | Type | Rules |
|-------|------|--------|
| `id` | string | Job id |
| `kind` | string | As reported (`migration`, `transform`, `backup`, `restore`, or an unknown raw kind) |
| `target` | string | Namespace/container or other target the job surface reports |
| `status` | string | Raw status. Unknown values pass through |
| `progress` | object \| null | `bytes_copied`, `bytes_remaining` when `010` has them. Null → "unavailable" |
| `failure` | string \| null | Present when the job surface reports a failure |

Cancel and resume call `POST /v1/jobs/{id}/cancel` and `/resume` after confirmation. The next view is a re-read, not the request body echoed as success before the response.

## Guarded Action

Ephemeral. Not stored. The shell builds it before a write.

| Field | Type | Rules |
|-------|------|--------|
| `action` | string | `namespace_create`, `namespace_rename`, `namespace_delete`, `quota_replace`, `config_reload`, `job_cancel`, `job_resume`, `node_drain`, `node_undrain`, `cluster_stop` |
| `target` | string | Operator-selected. Not pre-filled for drain or cluster stop |
| `effect` | string | Plain-language impact. Drain and cluster stop use the spec's impact sentences |
| `strength` | `normal` \| `strong` | `strong` for `node_drain` and `cluster_stop`. Strong requires typing `target` (node name, or the word `cluster`) |
| `outcome` | `idle` \| `unconfirmed` \| `accepted` \| `refused` | `accepted` only after HTTP success. `refused` carries the server `message`. `unconfirmed` after a transport drop, followed by a re-read |

Dismissing the dialog leaves `outcome` at `idle` and sends no request.

## Refusal / Error Presentation

Mapped from HTTP. The shell prints `message` when present and keeps the rest of the page usable.

| `code` | HTTP | Meaning |
|--------|------|---------|
| `unauthorized` | 401 | Login credential rejected ("invalid admin token") |
| `session_invalid` | 401 | Missing, expired, or foreign session cookie. Distinct copy from a bad password |
| `csrf_rejected` | 403 | Missing cookie on a cookie-only mutation, foreign or missing `Origin`, or bad CSRF proof. No state change. Distinct from validation |
| `forbidden` | 403 | Authenticated principal lacks the verb or the namespace. No foreign tenant body |
| `limit_exceeded` | 400 | `limit` outside 1–100 |
| `CascadeRequired` | 400 | Delete without cascade while containers remain. `message` is the `007` text |
| `deadline_exceeded` | 504 | Composer exceeded 1.5 s. Panel-level, not a blank page |
| validation codes from `007` / `010` / `011` | 4xx | Shown verbatim in `message` |

Strings from these bodies and from entity fields are text, never markup.

## Audit page row

Subset of `014` `AuditEntry` returned by `audit_list`. See [contracts/audit-list.md](contracts/audit-list.md).

| Field | Type | Rules |
|-------|------|--------|
| `id` | UUID | Cursor key, descending |
| `principal` | string | `login_at_event` when set, otherwise principal id, otherwise empty for an unknown login. Never a secret |
| `action` | string | `014` action |
| `target` | string | `014` target |
| `time` | RFC3339 | Rendered from the entry HLC when the clock crate can project it; otherwise the raw HLC is not shown as a friendly timestamp — the UI shows the RFC3339 if known and "time unavailable" if the HLC cannot be projected. It does not invent a clock |
| `namespace` | string \| null | Present for namespace-scoped entries the caller is allowed to see |

No retention, export, or configuration fields exist on this row or on the page.
