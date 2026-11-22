# Research: Modern Admin Web Console

**Feature**: `018-admin-ui-modernization` | **Date**: 2026-11-22

Each item is a Decision, a Rationale, and the Alternatives considered. Inputs: [spec.md](spec.md) (clarify 2026-10-31 and 2026-11-22), constitution 1.3.0, the current `crates/admin-ui` router, `014` `SessionToken` / `AuditEntry`, `015` `max_result`, and `011` `drain` / `undrain`. Uncommitted CSS and login polish under `crates/admin-ui` is baseline only. It is not the design.

No Technical Context item remains `NEEDS CLARIFICATION`.

## R1. One crate, maud, checked-in assets

- **Decision**: Extend `crates/admin-ui` (`spacestorage-admin-ui`). Pages are `maud` (already in that crate's `Cargo.toml`). Behavior and theme live in `assets/shell.js` and `assets/shell.css`, compiled in with `include_str!`. No new crate, no npm, no CDN, no WASM UI framework.
- **Rationale**: Constitution I and III, CC-001, NFR-008, NFR-009. The crate already serves `/ui/*` from `admin-http`. A second frontend project would be a second toolchain and a second process.
- **Alternatives considered**: A new `admin-shell` crate (rejected: splits one operator surface). `askama` (rejected: `maud` is already the dependency). Leptos/Yew (rejected: heavier, still needs a browser asset pipeline). npm SPA (rejected: constitution I).

## R2. Shell routes and permanent redirects

- **Decision**: The shell owns `/ui`. Section paths:

  | Path | Section |
  |------|---------|
  | `/ui/overview` | Cluster overview |
  | `/ui/namespaces`, `/ui/namespaces/{name}` | Namespace list and detail |
  | `/ui/nodes`, `/ui/nodes/{id}` | Node list and detail |
  | `/ui/monitoring`, `/ui/monitoring/namespaces/{name}` | Metrics |
  | `/ui/jobs`, `/ui/jobs/{id}` | Job progress |
  | `/ui/audit` | Audit |
  | `/ui/map` | Hosts the `009` cluster map |
  | `/ui/browse` | Hosts the `009` configure/browse console |

  Permanent redirects (`308`, query string preserved):

  - `GET /ui` → `/ui/overview`
  - `GET /ui/cluster` → `/ui/map`
  - `GET /ui/console` → `/ui/browse`

  Every section is one shell document. The request path is exposed as `data-path` on `<body>`. Filters (`q`, `status`, `namespace`) are copied into the query string so refresh, bookmark, and back/forward restore them. Unauthenticated responses are the login document only: no nav, no cluster payload, and `shell.js` does not call a data URL until `GET /v1/auth/session` returns 200.
- **Rationale**: FR-006 and clarify Q6. Distinct paths keep `009`'s old URLs as redirects instead of overloading `/ui/console` with two meanings. One HTML document avoids embedding tenant data in the first byte stream.
- **Alternatives considered**: Leave `/ui/cluster` and `/ui/console` as the `009` pages beside a new `/ui/app` (rejected: clarify says the shell owns `/ui` and those two paths redirect). Hash routing (rejected: not bookmark-stable without the path). Iframes (rejected: NFR-007 `frame-ancestors 'none'`, and a frame would be a second document the shell does not control).

## R3. How `009` is linked

- **Decision**: `/ui/map` and `/ui/browse` are shell pages. They load the existing `cluster-map.js` and `console.js` into a mount point in the shell document. Those scripts gain a one-shot render entry. `shell.js` is the only timer and calls that entry on the 10 s tick. The JSON contracts stay `009`'s: `GET /v1/cluster/map` and `POST /v1/console/query|config|containers`. The shell does not reimplement the graphic, the SQL path, or ingest.

  The map page does not request an unbounded topology. It draws `GET /v1/cluster/map?namespace={name}` for one namespace the principal may see. When the operator has not chosen a namespace, the page shows the namespace picker (paged, R5) and degraded callouts from the overview composer, not the full cluster document. Namespace detail links to `/ui/browse?namespace={name}` for authoring and row browse, and to `/ui/map?namespace={name}` for shards and replicas.

  User-visible headings in the hosted assets use container, shard, and replica. The engines and JSON field names stay as `009` defined them (`ContainerMapEntry`, `shards`, `replicas`).
- **Rationale**: Spec product goal and FR-015. `009` already accepts `?namespace=`. Hosting the script keeps one graphic. A 2 s timer inside the asset would violate FR-010 once the old pages redirect away.
- **Alternatives considered**: Duplicate the map in the shell (rejected: second topology view, CC-008). Keep the 2 s poll inside `009` assets (rejected: FR-010). Fetch the full map for 5 000 containers into an HTML table (rejected: large-fleet edge, NFR-003).

## R4. Session cookie, Origin, and CSRF

- **Decision**: `POST /v1/auth/login` verifies the admin credential the way it does today, issues the same 32-byte `014` session secret (SHA-256 stored, TTL 12 h, no idle timeout), and returns it only as:

  `Set-Cookie: ss_session=<hex>; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=43200`

  The JSON body has `expires_in`, `expires_at`, `principal`, `scope`, `namespaces`, and `csrf_token`. It has no `token` field.

  `csrf_token` is a separate 32-byte hex stored on the session record. It is not the session secret. Login mints it once. GETs do not rotate it (parallel panels would otherwise invalidate each other). `GET /v1/auth/session` returns the same identity block plus `csrf_token` when the cookie resolves.

  Cookie-authenticated methods other than GET, HEAD, and OPTIONS must, before any state change:

  1. Present a resolvable, unexpired `ss_session` cookie.
  2. Present an `Origin` whose host and port equal the request `Host`. Scheme is `https`, or `http` only when the host is `localhost` or `127.0.0.1`. A missing or foreign `Origin` is `403` `csrf_rejected`.
  3. Present header `X-SpaceStorage-CSRF` equal to the session's `csrf_token` (constant-time compare). A missing or wrong proof is `403` `csrf_rejected`.

  `Authorization: Bearer` equal to the node admin token file remains the CLI credential and does not require `Origin` or CSRF, because the browser does not attach it automatically. A session hex presented as `Authorization: Bearer` is `401` `session_invalid`. There is no `sessionStorage` fallback.

  `POST /v1/auth/logout` is a mutating route (cookie + Origin + CSRF). It revokes the session hash and sets `ss_session` with `Max-Age=0`.

  `Secure` is always set. Evergreen browsers store a `Secure` cookie on loopback HTTP. A remote plaintext admin listener will not keep the cookie; the browser console on a non-loopback host needs TLS. Conformance clients may send the `Cookie` header directly and must still observe `Secure` on `Set-Cookie`.

  After login the shell returns to a relative `next` path that starts with `/ui/` and contains no scheme, slash-slash, or backslash. Anything else becomes `/ui/overview`.
- **Rationale**: FR-002a, CC-007, clarify Q3. Putting the secret only in an `HttpOnly` cookie removes the `sessionStorage` XSS read. `SameSite=Strict` is necessary and not sufficient: the spec also requires `Origin` and a CSRF proof. Keeping the admin-token bearer path preserves `spacestorage` CLI calls that never held a browser session. Rejecting the session hex as a bearer closes the old UI fallback. Stable CSRF for the 12 h life avoids breaking the 10 s poll.
- **Alternatives considered**: Double-submit cookie as the only proof (rejected: a readable cookie is a weaker binding than a server-side value tied to the session hash; Origin is still required either way). Rotate CSRF on every response (rejected: overlapping polls). Require CSRF on the admin-token bearer (rejected: breaks the CLI and is not the browser ambient-credential threat). Return the session hex in JSON "for the CLI" (rejected: the login page's `fetch` could read it, which FR-002a forbids). Drop `Secure` on plaintext (rejected: the spec requires `Secure`).

## R5. Page size against `015`

- **Decision**: Namespace lists, namespace container inventories, node lists, job lists, and `audit_list` share one page contract:

  - Query `limit`, default **50**, maximum **100**.
  - `limit` above 100, zero, or non-numeric → `400` `limit_exceeded` with `limit`, `current`, and `max`. No silent clamp.
  - Opaque `cursor`. Omit or empty means the first page. The response `next_cursor` is null on the last page.
  - Serialized JSON of one page ≤ **256 KiB**. If the next row would pass that budget, close the page and set `next_cursor`. The client can continue. Rows are not dropped without a cursor.
  - Ordering is stable: namespaces and nodes by name, jobs by id, audit by `AuditEntry.id` descending (UUID v7 time order).

  `015` `max_result` is a **64 MiB** byte ceiling on data-plane results, not a row count. The `009` console already caps browse at 1 000 rows and 1 MiB (`MAX_PAGE_BYTES`, `MAX_PAGE_ROWS`). These admin pages sit strictly under both: 100 rows and 256 KiB is under 1 MiB and far under 64 MiB. The large-fleet edge (500 namespaces) cannot be rendered as one table.
- **Rationale**: The spec deferred this sizing to plan and required paging inside `015`. A named reject matches `015` (no silent truncation). Default 50 fits one operator screen and keyboard paging. Max 100 forces pagination on the 500-namespace edge without making every small cluster click through extra pages.
- **Alternatives considered**: Reuse the console max of 1 000 rows (rejected: a 500-namespace fleet would still arrive in one payload, which the spec forbids). Page size 20 (rejected: needless clicks on the reference cluster of 50 namespaces). Offset/limit without a cursor (rejected: a concurrent create or delete shifts rows; a cursor tied to the sort key does not).

## R6. Per-namespace container counts

- **Decision**: Do not add a maintained counter on the namespace record. Counts come from one bounded scan of `ContainerCatalog` descriptors (`list()`), metadata only, never row bytes.

  - Cap **10 000** descriptors per UI request. The read lock is dropped before any `.await`.
  - The scan tallies only namespaces on the current page (≤ 100) plus the overview total.
  - If the scan finishes under the cap, `container_count` is exact and `container_count_complete` is true.
  - If the cap is hit, `container_count_complete` is false and the UI copy is "at least {container_count}". The number is never shown as exact.
  - Cascade confirmation uses that same figure. An incomplete count is worded "at least N". The server's existing `CascadeRequired` / `namespace_delete` path stays authoritative.
  - Namespace **count** on the overview is the length of the controller namespace list already in memory (`007`), not a catalog scan and not a new counter. The HTTP body carries the integer, not the full name list.
  - Container **inventory** on namespace detail is a separate paged descriptor list (R5), not an expansion of the count into every name.
- **Rationale**: CC-002 forbids an unbounded scan to paint a page, and the spec allows exactly three surface extensions (cookie, `audit_list`, drain/undrain). A counter would be a fourth writer on `007` and could drift from the catalog. The reference cluster (500 containers) and the stated edge (5 000) both finish under the 10 000 cap, so those fixtures are exact. Beyond the cap the UI tells the truth.
- **Alternatives considered**: Maintained counter incremented on create/drop (rejected: new source of truth, easy to miss dual-write and restore). Unbounded `list()` on every overview poll (rejected: CC-002, and the poll is every 10 s). Hide container counts until a counter exists (rejected: FR-011 requires the count; the catalog already has the descriptors).

## R7. `audit_list`

- **Decision**: Add `AdminOp::AuditList { cursor, limit, namespace }` on `POST /v1/admin` and a shell read `GET /v1/ui/audit` that calls it. The read is paginated with R5. Gate: `CLUSTER_ADMIN`, or `AUDIT_READ`. A namespace-scoped principal with `AUDIT_READ` receives only entries whose `namespace_id` is theirs. `CLUSTER_ADMIN` receives every entry. Callers without either verb get `403` and an empty body. The response fields are principal (login or id as `014` stored them), action, target, and time. Key material is never copied. The shell offers no retention, export, or audit-configuration control. This feature does not invent a second log. If the cluster-log apply path has no rows yet, the page is empty and says so.
- **Rationale**: FR-021b and `014` contract "Read". The spec draws this query into plan scope because the admin enum has no list operation today (`AdminOp` ends at `Membership`). Pagination is the spec's whole constraint on the new query.
- **Alternatives considered**: Stream the entire audit log to the browser (rejected: NFR-003). A UI-private ring buffer (rejected: CC-006 / `014` source of truth). Export buttons (rejected: out of scope).

## R8. Drain and undrain on the admin path

- **Decision**: Add `AdminOp::Drain { node_id }` and `AdminOp::Undrain { node_id }`. The node handler calls a thin `MembershipService::operator_drain` / `operator_undrain` that delegates to `membership::drain` / `undrain` and records the `MembershipEvent` those functions return. `011` stays the only membership writer. The shell never mutates `MemberRecord` itself.

  Drain confirmation is the strong form: the dialog names that node, states that new tenant connections and new placements stop and that replicas move off before decommission, and requires the operator to type the node name. The field is not pre-filled. Undrain uses a normal confirmation naming the node and stating that it may accept placements again. Cluster stop stays `POST /v1/stop` and uses the strong form with the operator typing `cluster`. Decommission and replace stay CLI-only.
- **Rationale**: Spec assumption (iii) and FR-021a. `drain` / `undrain` exist as pure functions and are not on `AdminOp` today, so the console cannot call them. Semantics stay in `crates/membership/src/drain.rs`.
- **Alternatives considered**: A new `/v1/membership/drain` resource that bypasses `AdminOp` (rejected: CLI and UI would diverge; one admin enum is enough). Shell-side status write (rejected: CC-006). Bundling decommission into the same dialog (rejected: out of scope).

## R9. Composed reads under `/v1/ui`

- **Decision**: Browser reads go to cookie-authenticated `GET /v1/ui/...` composers in `admin-ui`, implemented by calling existing services. The browser does not receive a cluster-wide admin dump and filter it. Writes from the shell go to the owning routes, now accepting the cookie: `POST /v1/admin` (namespace, quota, drain, undrain), `POST /v1/reload`, `POST /v1/stop`, `POST /v1/jobs/{id}/cancel`, `POST /v1/jobs/{id}/resume`. Each composer returns `generated_at` (RFC3339, `Z`) and per-panel `status` of `ok` or `unavailable` so one dependency failure does not blank the shell (FR-022, NFR-004). Controller-backed figures set `source` to `controller` or `node-local`. A single-node cluster with no roster still returns the local node.
- **Rationale**: CC-004 and FR-026. Composing on the server is what makes "hidden in the browser" impossible. Reusing write routes avoids a second mutation API.
- **Alternatives considered**: Let `shell.js` POST raw `AdminOp::NamespaceList` and slice the JSON (rejected: the payload would include other tenants before the script ran). A GraphQL gateway (rejected: new engine, CC-008).

## R10. Freshness

- **Decision**: `shell.js` polls each visible panel every 10 s, waits for the in-flight request before starting another, and schedules nothing while `document.visibilityState !== "visible"` or the operator has paused. On becoming visible after a hidden period, it fetches immediately and, until that fetch returns, marks panels stale if their `generated_at` is older than 30 s. Every panel prints the age and the word "stale" when the age exceeds 30 s. Status is never color alone: a text label accompanies any colored mark. Timestamps render as absolute RFC3339 and a relative age ("30 s ago").
- **Rationale**: Clarify Q2, FR-009, FR-010. The spec refused a push stream.
- **Alternatives considered**: Server-sent events (rejected: clarify Q2). Keep the `009` 2 s map timer (rejected: R3). Poll even when the tab is hidden (rejected: FR-010, SC-005).

## R11. CSP, accessibility, and the single theme

- **Decision**: HTML and JSON responses that the shell and its assets use carry:

  - `Content-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'`
  - `X-Content-Type-Options: nosniff`
  - `Referrer-Policy: same-origin`

  No inline script and no inline style, so `unsafe-inline` is absent. Cluster strings are inserted with `textContent` or `maud`'s escaped text. One light theme, English copy, no branding asset, no webfont. Primary flows use `<main>`, `<nav>`, headings, labelled inputs, and a visible `:focus-visible` outline. Contrast pairs live in `shell.css` and meet WCAG 2.1 AA for text and non-text controls. Automated checks assert structure and the presence of non-color status text. A full axe run is out of tree because it would pull a second toolchain; the quickstart lists the keyboard pass.
- **Rationale**: FR-023, FR-024, NFR-006, NFR-007, assumptions Q4/Q8/Q9. Current pages inline their script, which cannot satisfy this CSP. New shell documents do not.
- **Alternatives considered**: `unsafe-inline` to keep the current login script in the HTML (rejected: weakens NFR-007). A dark theme (rejected: assumption Q4). npm `@axe-core` (rejected: NFR-009).

## R12. Guarded actions and honest outcomes

- **Decision**: The shell does not apply a write to the view until the HTTP call returns success. A dismissed dialog sends nothing. A dropped connection shows "unconfirmed" and re-reads. Server refusal is shown with the response `message` next to the form, and the previous rows stay. Strong confirmations (drain, cluster stop) require typed acknowledgement of the target the operator selected. The target is not a hidden default. Namespace delete names the namespace and the container count (R6). Quota replace names the namespace and the proposed quota. Job cancel and resume name the job id and target. Reload names configuration reload and that the running config is replaced by the last validated file. Affordances in `GET /v1/auth/session` hide controls the principal cannot use; the routes still enforce the verb.
- **Rationale**: FR-021, FR-021a, SC-007, SC-012, and the "no silent last-write-wins" edge. Optimistic UI would report success for a failed write.
- **Alternatives considered**: Optimistic removal from the namespace list (rejected: SC-012). A single shared "Are you sure?" with no target name (rejected: FR-021a). Client-only disable of cluster stop for a namespace admin (rejected: FR-003).
