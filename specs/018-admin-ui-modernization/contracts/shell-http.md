# Contract: Admin shell HTTP

**Feature**: `018-admin-ui-modernization` | Router: `admin-http` (`001`), pages from `crates/admin-ui`

Authentication for HTML is described in [session-cookie.md](session-cookie.md). JSON reads are [ui-read-models.md](ui-read-models.md). Map and console JSON stay [009's ui-http contract](../../009-admin-ui-ingest/contracts/ui-http.md), with the cookie accepted as the browser credential (R4 in [research.md](../research.md)).

Application label on existing `008` API series: `spacestorage-ui`. Protocol label: `admin-http`. No new label keys.

## HTML

Unauthenticated `GET` of any `/ui/...` section returns **200** the login document only: no `<nav>` to data sections, no namespace, node, metric, job, or audit text, and the script does not fetch `/v1/ui/*`.

Authenticated `GET` returns **200** the shell document. The body carries `data-path` equal to the request path. It may include the UI Session View (principal, scope, expiry). It must not embed namespace lists, node rows, series, jobs, or audit rows. Those load from `/v1/ui/*` after `GET /v1/auth/session` succeeds.

| Method | Path | Success |
|--------|------|---------|
| GET | `/ui` | **308** `/ui/overview` |
| GET | `/ui/cluster` | **308** `/ui/map` (query string preserved) |
| GET | `/ui/console` | **308** `/ui/browse` (query string preserved) |
| GET | `/ui/overview` | 200 shell, section overview |
| GET | `/ui/namespaces` | 200 shell |
| GET | `/ui/namespaces/{name}` | 200 shell. Data request may still 403 |
| GET | `/ui/nodes` | 200 shell. Filters `q`, `status` live in the query |
| GET | `/ui/nodes/{id}` | 200 shell |
| GET | `/ui/monitoring` | 200 shell |
| GET | `/ui/monitoring/namespaces/{name}` | 200 shell |
| GET | `/ui/jobs` | 200 shell |
| GET | `/ui/jobs/{id}` | 200 shell |
| GET | `/ui/audit` | 200 shell |
| GET | `/ui/map` | 200 shell hosting the `009` map script |
| GET | `/ui/browse` | 200 shell hosting the `009` console script |
| GET | `/ui/assets/{*name}` | 200 asset, or 404. Names: `shell.js`, `shell.css`, `cluster-map.js`, `console.js`, `console.css`, `ui.css` |

`308` responses include a `Location` on the same host and a short HTML body that links to it, so a non-automatic client can follow. Redirects do not require a session: they only change path. The destination then applies the login rule.

A missing asset does not replace the document with an empty body. The shell document is readable without the script and states that interactivity is limited when the script fails to run (noscript or failed load). That sentence is in the HTML, not only in JS.

## Vocabulary

Navigation labels and primary headings:

| `id` | Label |
|------|--------|
| `overview` | Overview |
| `namespaces` | Namespaces |
| `nodes` | Nodes |
| `monitoring` | Monitoring |
| `jobs` | Jobs |
| `audit` | Audit |
| `map` | Cluster map |
| `browse` | Browse |

Empty states, confirmations, and errors use namespace, container, shard, replica, job, and principal. They do not use index, document, ILM, or data stream as the operator's noun. The map page's graphic talks about containers, shards, and replicas.

## Cluster map and browse hosting

`/ui/map` loads `/ui/assets/cluster-map.js` into a mount inside the shell. The script exposes a one-shot render. It does not start its own interval when the shell sets `window.SS_SHELL = true` before the script runs. The shell asks the map to render from `GET /v1/cluster/map?namespace={name}` for a namespace the session allows. With no namespace selected, the page shows the paged namespace picker and does not request the unfiltered map.

`/ui/browse` loads `/ui/assets/console.js` the same way, against `POST /v1/console/query`, `/v1/console/config`, and `/v1/console/containers`. Those posts are cookie-authenticated mutations and carry `Origin` and `X-SpaceStorage-CSRF`.

No iframe. `frame-ancestors 'none'` is on every shell and asset response.

## Poll

`shell.js` is the only timer.

- Interval **10 s** per visible panel, one in flight at a time per panel.
- No request while `document.hidden` or while the operator's pause control is on.
- When the tab becomes visible, fetch immediately. Until it returns, a panel whose `generated_at` is older than **30 s** shows the text "stale" and the age.
- Every panel shows age as a relative phrase and the absolute RFC3339 time.
- Pause is a button, keyboard-reachable, with an accessible name.

## Security headers

On shell HTML, `/v1/ui/*`, `/v1/auth/*`, and `/ui/assets/*`:

```http
Content-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'
X-Content-Type-Options: nosniff
Referrer-Policy: same-origin
```

No `unsafe-inline`. Shell script and style are files under `/ui/assets/`. Cluster data is set with text APIs, not `innerHTML`.

## Accessibility

The shell document includes `header`, `nav`, `main`, and a heading hierarchy. Tables have header cells. Inputs have labels. Focus uses `:focus-visible`. Ready, not ready, pending, unknown, stale, and failed each have a text label, not only a color.

Viewport: layout works from 768 px wide and at 200% zoom without a horizontal scrollbar on Overview, namespace create, and node detail.

## Errors

A `401` `session_invalid` on a data call replaces the shell with the login document and keeps `next` as the path the operator was on. Form fields the operator had typed stay in the document; the shell does not mark them submitted.

A `403` `forbidden` on a section's data call renders a refusal in `main` and leaves nav usable. The refusal body has no other tenant's names.

A `504` `deadline_exceeded` or a network drop marks that panel unavailable and leaves the others as they were. A drop during a write sets the action outcome to unconfirmed and re-reads.
