# Feature Specification: Modern Admin Web Console

**Feature Branch**: `018-admin-ui-modernization`

**Created**: 2026-10-31

**Status**: Converged clean (2026-11-22)

**Input**: User description: "creating a modern UI web interface enough to administrate clusters, create namespaces and review status/monitoring and nodes data"

## Scope Note

This feature modernizes the **web surface** of the admin console: a coherent navigation shell, a cluster overview, namespace lifecycle management, node inventory and node detail, and status/monitoring review. It is a presentation and workflow layer over surfaces that already exist or are owned by other specs:

- authentication, session, permission vocabulary, audit — `014`, with two scoped extensions owned by this feature's plan (see **Cross-feature change to `014`** below): server-set session-cookie issuance with `Origin`/CSRF verification on mutating admin routes, and a read-only paginated `audit_list` query;
- admin status/config/threads/buffers/reload/stop, node drain, and job surfaces — `001`, `010`, `011`;
- controllers, Raft status, membership — `006`, `011`;
- namespaces, quotas, roles/bindings — `007`;
- metrics and labels — `008`;
- the Cerebro-class cluster map and the Kibana-class browsing/console and log ingest — `009`.

`009` remains the owner of the cluster-map graphic, the configure/browse console, and ingest. This feature MUST NOT re-specify ingest, MUST NOT invent a second query engine, and MUST NOT duplicate `009`'s browsing views. It provides the modern shell, the administrative views the shell navigates to, and the entry point that links to `009` where those views already exist.

### Product goal — Cerebro / Kibana class, SpaceStorage entities

The product goal for the admin web surface (this shell **plus** the `009` map and console it links to) is **operator capability in the class of Cerebro and Kibana**, not a visual or vocabulary clone of those Elasticsearch tools. Every primary surface MUST be framed in **SpaceStorage** terms:

| Operator need (ES-class reference) | SpaceStorage entity / surface |
|------------------------------------|-------------------------------|
| Cluster health and topology (Cerebro) | Cluster readiness, membership roster, pending joins, controllers / Raft |
| Node inventory and node detail (Cerebro) | Members: identity, readiness, threads, buffers, topology labels |
| Index / shard map (Cerebro) | **Containers** and their **shards / replicas**, placement and replication lag — never Elasticsearch "indices" or "documents" as product nouns |
| Stack / cluster management (Kibana) | This feature's shell: Overview, **Namespaces**, Nodes, Monitoring, jobs, audit, guarded admin writes |
| Discover / browse (Kibana) | `009` console: browse **containers** and datatype-level views via `005` |
| Dev Tools / configure (Kibana) | `009` console: configure cluster and per-**namespace** settings; ad-hoc work through existing execution, not a private query engine |
| Ingest pipelines (Kibana-adjacent) | `009` ingest (Kafka / syslog) — CLI/HTTP; not re-owned here |

UI copy, navigation labels, empty states, and confirmations MUST use SpaceStorage nouns (namespace, container, shard, replica, job, principal). Borrowed ES words (index, document, ILM, data stream as ES products) MUST NOT appear as the console's primary vocabulary. Visual fidelity to Cerebro or Kibana chrome is **not** required; functional coverage over the mapping above **is**.

**Cross-feature change to `014`.** Session issuance is `014`'s, and today it returns a 12 h bearer token that the page keeps in `sessionStorage`. This feature draws exactly two changes to that owned surface into its planning scope: (a) the login response sets an `HttpOnly; Secure; SameSite=Strict` session cookie instead of handing a script-readable token to the page, and every mutating admin route verifies `Origin`/CSRF; (b) a read-only, paginated `audit_list` query gated by `AUDIT_READ` or `CLUSTER_ADMIN`. Nothing else about `014` changes — the fixed 12 h lifetime with no idle timeout, the permission vocabulary, and the audit records themselves stay as `014` defines them. `014`'s spec and plan must be reconciled with (a) and (b) before the console ships; this feature does not redefine them.

## Clarifications

### Session 2026-11-22

- Q: Are Cerebro and Kibana literal UI targets, or a capability class — and in whose vocabulary? → A: Capability class only. The admin web surface (this shell plus linked `009` map/console) aims at Cerebro/Kibana-class operator workflows, expressed exclusively in SpaceStorage entities (cluster, namespace, container, shard, replica, job, principal). No Elasticsearch product nouns as primary UI vocabulary; no requirement to look like Cerebro or Kibana. `009` keeps ownership of map/console/ingest realization; this feature owns the management shell and the navigation that makes those views part of one console.

### Session 2026-10-31

- Q: Does the console perform destructive cluster operations, or is it read-only plus safe create/rename? → A: Full write parity with the CLI admin surface — namespace create, rename, and delete (cascade), configuration reload, quota replace, job cancel and resume, node drain, and cluster stop. Node drain and cluster stop carry the strongest confirmation UX: the dialog names the specific node or the whole cluster and states the expected impact (containers/shards moved or evicted for a drain; every service on the cluster stopping for a cluster stop). The safety and acceptance matrix MUST include both the refusal and the confirmation path for each.
- Q: How does a panel learn about changes — fixed-interval polling, or a server-pushed stream? → A: Bounded polling every 10 s. Data is marked stale once its age exceeds 30 s, every panel shows its data age, the operator can pause refresh, and refresh stops while the tab is hidden.
- Q: How is the browser session carried — the existing session token held by the page, or a server-set `HttpOnly` cookie? → A: A server-set `HttpOnly; Secure; SameSite=Strict` cookie, with an `Origin`/CSRF check on every mutating route; the session token is never readable by page script. This is a coordinated change with `014`, which owns session issuance and today hands a 12 h bearer token to `sessionStorage`: the `014`-owned surface change — cookie issuance plus `Origin`/CSRF verification on mutating admin routes — is drawn into this feature's plan scope. The session lifetime stays `014`'s: a fixed 12 h token, no idle timeout, inherited unchanged.
- Q: Where does the console's audit view get its data — a new query surface, or an existing one? → A: A minimal read-only, paginated `audit_list` query added to the `014`-owned admin surface, gated by `AUDIT_READ` / `CLUSTER_ADMIN`, and rendered by the console. The view is render-only: no retention, export, or audit configuration changes from the UI.
- Q: Does the shell replace `/ui/cluster` and `/ui/console`, or sit beside them? → A: The new shell owns `/ui`; `/ui/cluster` and `/ui/console` permanently redirect into the shell's corresponding section, which links through to — or, only where `009` already exposes fragments, embeds — the `009`-owned views. Existing bookmarks resolve; `009` keeps ownership of the map and console content.

**Deferred to `/speckit-plan`** (deliberately not decided here, and not blocking any behaviour stated in this spec): the page size the namespace and audit listings use relative to `015` result limits, and whether per-namespace container counts are computed as bounded scans or read from a maintained counter. Both are sizing decisions for planning.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Sign in to a coherent admin shell (Priority: P1)

A cluster administrator opens the admin UI on any node, signs in with the `014` credential, and lands in a single shell with persistent navigation: Overview, Namespaces, Nodes, Monitoring, plus links to the cluster map and console owned by `009`. The shell always shows who is signed in, what scope they hold, and when the session expires. Nothing about the cluster is rendered before the session is verified. The session travels in a server-set `HttpOnly; Secure; SameSite=Strict` cookie that page script cannot read, and every mutating route verifies `Origin`/CSRF before acting. Signing out clears the browser's session. Refusals are explained, not silent.

**Why this priority**: Every other view depends on a trustworthy shell and session. A modern console that leaks data before authentication or hides refusals is worse than no console.

**Independent Test**: Load the UI without a session, confirm only the login surface is served and no cluster payload is requested or rendered; sign in as a cluster admin, confirm navigation, identity, and that page script cannot read the session credential; issue a write with a foreign `Origin` and confirm it is refused; sign in as a namespace admin and confirm the shell narrows to that namespace; force session expiry and confirm the next action returns to login with the attempted destination preserved.

**Acceptance Scenarios**:

1. **Given** no verified session, **When** any UI route is opened, **Then** only the sign-in surface is rendered, no namespace, node, metric, or container data appears, and no privileged data request is issued.
2. **Given** a valid `014` session for a cluster administrator, **When** the shell loads, **Then** the identity, effective scope (cluster-wide), and session expiry are visible, and every primary section is reachable.
3. **Given** a valid session for a principal that is only a namespace admin, **When** the shell loads, **Then** cluster-wide sections are absent or clearly disabled, the principal's namespace is selected, and no other tenant's data is present in the payload.
4. **Given** a signed-in principal, **When** they sign out, **Then** the session cookie is cleared, the shell returns to the sign-in surface, and a reload does not restore the session.
5. **Given** an expired session, **When** the operator triggers any data action, **Then** the action is refused as unauthenticated, the shell returns to sign-in, and after signing in the originally requested view is restored.
6. **Given** a direct link to a view the principal may not use, **When** it is opened, **Then** an explanatory refusal is shown and no data from that view is present in the response.
7. **Given** a signed-in session, **When** a mutating request arrives without the session cookie, with a foreign `Origin`, or without a valid CSRF proof, **Then** it is refused before any state changes and the refusal is shown.
8. **Given** a signed-in session, **When** page script inspects cookies and browser storage, **Then** it finds no readable session credential, and the console continues to work without one.

---

### User Story 2 - Cluster overview and health at a glance (Priority: P1)

An operator lands on an overview that answers, without drilling: is the cluster ready, which nodes are not ready, how many namespaces and containers exist, are any replicas degraded or lagging, and are any migration or maintenance jobs failing. Degraded conditions are called out above the fold rather than buried in a table. Each panel states how old its data is: panels poll every 10 s, mark themselves stale after 30 s, and a paused or stale view is never presented as current.

**Why this priority**: This is the core reason the operator opens the UI. It is independently valuable even if namespace and node management never ship.

**Independent Test**: On a three-node cluster, mark one member not ready and induce lag on one replica; open the overview as a cluster admin and confirm both conditions are visible above the fold with the affected node/replica named; pause refresh and confirm the data age is shown and the stale state is explicit.

**Acceptance Scenarios**:

1. **Given** a healthy three-node cluster, **When** a cluster admin opens the overview, **Then** cluster readiness, the number of ready and not-ready members, namespace and container counts, replica health, and active job counts are shown with the time of the last successful load.
2. **Given** one member is not ready and one replica is lagging, **When** the overview loads, **Then** both are surfaced above the fold, naming the node and the affected namespace/container, and the degraded state is distinguishable without relying on colour alone.
3. **Given** the control plane is reachable, **When** the overview renders topology-derived figures, **Then** the figures match the controller's view rather than a node-local guess.
4. **Given** refresh is paused or the last load is more than 30 s old, **When** the operator reads the page, **Then** the panel is marked stale with a visible age, and no value is presented as live.
5. **Given** the browser tab has been hidden for longer than 30 s, **When** it becomes visible again, **Then** the page does not silently show hours-old figures as current.
6. **Given** a single-node cluster with no membership roster, **When** the overview loads, **Then** the local node is shown rather than an empty or errored state.

---

### User Story 3 - Namespace lifecycle management (Priority: P1)

A cluster administrator lists namespaces, sees each one's container count and quota summary, and creates, renames, deletes, or replaces the quota of a namespace without leaving the UI. Validation and refusal messages come from the server and are shown verbatim enough to act on. Deleting a namespace that still holds containers is refused unless the operator explicitly confirms a cascade, and the confirmation names the namespace and the number of containers. A namespace administrator sees only their own namespace and cannot reach another one by editing the address.

**Why this priority**: Namespace creation is explicitly requested and is the first tenancy workflow that must not require the CLI.

**Independent Test**: As a cluster admin, create a namespace, confirm it appears without a manual reload, rename it, attempt deletion while it holds a container and confirm the refusal, then confirm cascade deletion; replace the quota and confirm the summary follows the server; as a namespace admin, confirm the list and every direct link expose only that namespace.

**Acceptance Scenarios**:

1. **Given** a cluster admin on an empty cluster, **When** they open Namespaces, **Then** an empty state explains that no namespaces exist and offers the create action instead of an error.
2. **Given** a valid new namespace name, **When** the cluster admin submits creation, **Then** the namespace is created through the `007` surface, appears in the list without a full page reload, and the creation is auditable per `014`.
3. **Given** a name that is invalid under `007` rules or already taken, **When** creation is submitted, **Then** it is refused and the server's reason is shown next to the field, and no namespace is created.
4. **Given** a namespace with containers, **When** the cluster admin requests deletion without cascade, **Then** the deletion is refused and the refusal states that containers remain.
5. **Given** a namespace with containers, **When** the cluster admin confirms cascade deletion by naming the namespace and acknowledging the container count, **Then** the deletion proceeds through the `007` surface and the view reflects the removal.
6. **Given** a namespace admin for `acme`, **When** they open the Namespaces view or a direct link to another namespace, **Then** only `acme` is visible and the other namespace is refused without disclosing its contents.
7. **Given** a rename, **When** it succeeds, **Then** the list and any open detail view reflect the new name, and links to the old name resolve to the renamed namespace or an explicit "not found", never to a different tenant.
8. **Given** any namespace mutation, **When** the request fails, **Then** the view keeps the previous state and shows the failure; it never shows success for a failed write.
9. **Given** a namespace with a quota, **When** the cluster admin confirms a quota replacement that names the namespace and the proposed quota, **Then** it proceeds through the `007` quota surface, the summary reflects the server's confirmed values, and an invalid quota is refused with the server's reason and no change applied.

---

### User Story 4 - Node inventory and node detail (Priority: P2)

An operator reviews every member of the cluster and each member's operational data: identity, name, readiness, role in the controller, and — on the detail view — state, worker-thread configuration, buffer usage, and the labels that place data. Pending joins are visible so an operator can tell "not yet admitted" from "lost". Values the server does not expose are shown as unavailable, never guessed or defaulted to healthy.

**Why this priority**: Node data is explicitly requested, but the overview already answers "is anything wrong"; per-node depth is the second step.

**Independent Test**: On a three-node cluster with one pending join, open Nodes as a cluster admin and confirm all members and the pending entry appear with accurate states; open a node detail and confirm threads, buffers, and labels match the admin status surfaces; open Nodes as a namespace admin and confirm only the nodes hosting that namespace's containers or shards appear; on a single-node cluster confirm the local node is listed.

**Acceptance Scenarios**:

1. **Given** a cluster with members and one pending join, **When** the Nodes view loads, **Then** each admitted member and each pending join is listed with a status that distinguishes ready, not ready, and pending.
2. **Given** a member whose readiness is unknown because the reporting node is unreachable, **When** the Nodes view renders it, **Then** it is shown as unknown, not as healthy.
3. **Given** a node detail is opened, **Then** the node's state, readiness, worker-thread configuration, buffer usage, and topology labels are shown, and each value is attributed to the node that reported it.
4. **Given** a field is not exposed by the current server surfaces, **When** the detail renders, **Then** the field is shown as unavailable with no fabricated value.
5. **Given** a namespace admin, **When** they open Nodes, **Then** the list shows only the nodes that host that namespace's containers or shards, with no cluster-wide capacity or placement data and no other tenant's containers, and a principal with no namespace visibility is not offered the Nodes section at all.
6. **Given** a cluster admin filters the node list by name or status, **When** the filter is applied, **Then** the list narrows without a full reload and the filter is reflected in the address so the view can be shared.

---

### User Story 5 - Monitoring review and job progress (Priority: P2)

An operator drills from the overview into monitoring: per-namespace metric views, replica and replication lag detail, and the progress of migration, transform, backup, and restore jobs. The UI renders the metric series and labels that `008` already defines; it does not invent, rename, or recompute them. A series that is unavailable is reported as unavailable, not as zero.

**Why this priority**: "Review status/monitoring" is part of the request, and reproduction of an incident needs drill-down, not only a summary.

**Independent Test**: With a running migration and a namespace with traffic, open Monitoring as a cluster admin, confirm the namespace metric view matches the `008` namespace surface, confirm the job shows status and progress, and confirm a deliberately unavailable series reads as unavailable.

**Acceptance Scenarios**:

1. **Given** a namespace with recorded metrics, **When** a permitted principal opens the namespace monitoring view, **Then** the displayed series and labels correspond to the `008` surface for that namespace, with no renamed or synthesized series.
2. **Given** a metric source is unreachable or a series is absent, **When** the panel renders, **Then** it states that the series is unavailable, and the rest of the page remains usable.
3. **Given** a running migration or transform job, **When** the operator opens job progress, **Then** the job's status and progress are visible and update as the job advances, and a failed job shows its failure state.
4. **Given** a principal without the metrics permission for the requested scope, **When** they request a metric view, **Then** the request is refused and the refusal is shown; no partial series are rendered.
5. **Given** a namespace admin, **When** they open monitoring, **Then** only their namespace's series are offered, and a cluster-wide request is refused.
6. **Given** the operator leaves a monitoring view open, **When** the data is more than 30 s old, **Then** the panel shows data age and does not claim to be live.

---

### User Story 6 - Guarded operational actions (Priority: P3)

An operator performs, from the console, the same writes the admin CLI offers: namespace create, rename, and delete (cascade), configuration reload, quota replace, job cancel and resume, node drain, and cluster stop — plus reading the audit trail where `014` allows it. Every action carries a confirmation step that names the target and states the effect, and the outcome is reported honestly. Node drain and cluster stop get the strongest confirmation: the dialog names the specific node or the whole cluster and spells out what will happen, so the operator cannot mistake which target is about to change. Actions a principal may not perform are absent or disabled, and are refused by the server if attempted directly.

**Why this priority**: Useful for day-two operations, but the console is already valuable without it. Full CLI write parity is in scope, so the safety work is sized for the most destructive actions rather than for reload alone.

**Independent Test**: As a cluster admin, trigger a reload and confirm the target and effect are stated before it runs and the result is reported; start a drain, confirm it does not run until the dialog names the node and impact, then confirm it; open the cluster-stop dialog and confirm it names the whole cluster; dismiss each dialog once and confirm no request is sent; attempt a cluster-scoped action as a namespace admin and confirm it is unavailable and refused when forced; cancel and resume a job; open the audit view as a principal without `AUDIT_READ` and confirm refusal, then as a principal with it and confirm entries come from `audit_list`.

**Acceptance Scenarios**:

1. **Given** a cluster admin, **When** they trigger a configuration reload, **Then** a confirmation names the target and the effect, and the outcome (accepted or refused) is reported with the server's reason.
2. **Given** a namespace-only principal, **When** they attempt a cluster-scoped action by any route, **Then** the action is refused server-side even if the control was reachable in the page.
3. **Given** a principal permitted to read audit, **When** they open the audit view, **Then** entries come from the read-only paginated `audit_list` query and show principal, action, target, and time as `014` records them; the view pages without requesting an unbounded collection and offers no retention, export, or configuration controls. A principal without `AUDIT_READ` is refused.
4. **Given** a confirmation is dismissed, **When** the dialog closes, **Then** no request is sent and no state changes.
5. **Given** an action whose result is unknown because the connection dropped, **When** the UI recovers, **Then** it reports the action as unconfirmed and re-reads state instead of claiming success or failure.
6. **Given** a ready member, **When** the cluster admin requests a drain, **Then** the confirmation names that node and states the expected impact (no new tenant connections or placements, replicas moved off before decommission), nothing is sent before confirmation, and the outcome from the membership surface is reported.
7. **Given** a cluster admin, **When** they request cluster stop, **Then** the confirmation names the whole cluster and states that every service on it stops, requires an acknowledgement beyond a single default button, and — once confirmed — reports the server's outcome; a namespace-scoped principal is neither offered the action nor able to invoke it directly.
8. **Given** a running or paused job, **When** the operator cancels or resumes it and confirms, **Then** the request goes through the owning job surface, the reported state matches the server's, and a refusal or failure is shown with its reason.

---

### Edge Cases

- **Empty cluster**: no namespaces and no containers produce explanatory empty states with the relevant create action, not errors or spinners.
- **Single-node cluster**: no membership roster exists; the local node is still listed and its detail is reachable.
- **Large fleet**: 500 namespaces, 5,000 containers, or 1,000 nodes must paginate or filter; the page must not attempt to render every row at once, and no request may ask the server for an unbounded collection.
- **Very long names, labels, or error text**: truncated for layout with the full value retrievable; the layout must not break.
- **Names with characters outside the `007` name rules**: refused with a validation message; the UI must not silently normalize a name into a different tenant.
- **Control plane unavailable**: views that depend on controller state either show node-local data explicitly labelled as such, or show that controller data is unavailable; neither may be presented as authoritative.
- **Partially unavailable node**: a node that does not answer must not make the whole Nodes or Overview page fail.
- **Session expiry during a form**: submission is refused as unauthenticated, the entered values are not silently lost, and no write is reported as successful.
- **Session cookie missing or blocked**: a request without the cookie is treated as unauthenticated; there is no fallback to a URL, storage, or page-held token.
- **Foreign `Origin` or missing CSRF proof on a write**: the request is refused before any state changes and reported as a security refusal, distinguishable from a validation error.
- **Drain or stop on the wrong target**: because these name a node or the whole cluster in the confirmation, a mistargeted action must be catchable at the dialog; the confirmation is never pre-filled with a default target the operator did not choose.
- **Two operators editing the same namespace**: a conflicting change is surfaced and state re-read; no silent last-write-wins illusion in the UI.
- **Back/forward after a delete or rename**: the view resolves to the current state or an explicit "removed/not found", never to a stale cached copy of deleted data.
- **Stale cookie from another cluster or an old session**: refused as unauthenticated, with a distinct message from "wrong password".
- **Missing or slow asset**: a failed script or stylesheet load must leave the page readable and explain that interactivity is limited, not render a blank page.
- **Metrics backend down**: monitoring panels degrade individually; the overview remains usable.
- **Unknown future node states or job kinds**: rendered as unknown with the raw identifier preserved; the UI must not crash or guess a friendlier state.
- **Timezone and clock skew**: timestamps are shown with an explicit zone, and relative ages are labelled as relative.

## Constitution-Informed Constraints *(mandatory for planning)*

These are non-negotiable product constraints, not implementation preferences. `/speckit.plan` MUST check them, and a violation needs Complexity Tracking or the change does not proceed.

- **CC-001 — Rust-only stack (Principle I).** The console MUST be built and served by this Rust workspace. There MUST be no mandatory Node.js/npm/bundler toolchain, no separate frontend service, and no runtime dependency fetched from a public CDN. Browser assets are static, vendored in the repository, and served by the server. A build that requires a second language ecosystem is a constitution violation.
- **CC-002 — Fully asynchronous, non-blocking (Principle II).** Every UI data path MUST run on the Tokio runtime and MUST NOT perform blocking database, filesystem, fsync, or control-plane work on an async worker thread. Reads that touch catalog, placement, controller, or job state MUST go through the shared async services; anything blocking MUST be scheduled off the worker pool. A UI request MUST NOT hold a lock across an await in a way that stalls the data path, and MUST NOT trigger an unbounded scan to render a page.
- **CC-003 — Single-process multithreaded monolith (Principle III).** The console MUST be served from the same single process as admin HTTP. No sidecar, proxy, or separate web tier is introduced. Any node must be able to serve it (Principle VI).
- **CC-004 — Multi-tenant namespaces (Principle IX).** Every view is either cluster-scoped or namespace-scoped. A namespace-scoped view MUST be filtered server-side, not only hidden in the browser, and MUST NOT include another tenant's counts, names, labels, or series.
- **CC-005 — Observability as a product surface (Principle X).** The UI renders the metric names, labels, and series defined by `008`; it MUST NOT rename, re-derive, or drop required labels, and MUST NOT introduce a parallel metric vocabulary. UI traffic itself MUST be attributable under the existing application label and existing label set.
- **CC-006 — Controller-owned metadata (Principle XII).** Namespaces, membership, roles, and placement truth come from the control plane. The UI MUST NOT maintain a second configuration or membership store, and MUST NOT present node-local state as controller state.
- **CC-007 — Security defaults (Principle XIII, `014`).** Authorization uses the closed `014` vocabulary. The UI has no private privilege model, no client-side-only enforcement, and no separate credential store. The session is carried in a server-set `HttpOnly; Secure; SameSite=Strict` cookie with `Origin`/CSRF verification on mutating routes; no script-readable token exists. Privileged actions are audit-logged by the owning surface.
- **CC-008 — No second type system or engine (Architectural Contracts, `009`).** The console MUST NOT become a second query engine, a second configuration store, or a second schema authority. Reads and writes go through the existing admin, tenancy, observability, and query surfaces.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The admin console MUST be served by the same process that serves the admin HTTP surface, under a stable, documented set of UI addresses, with no separate frontend service and no third-party origin required to load it.
- **FR-002**: The console MUST authenticate through the `014` session established at sign-in and MUST NOT render, request, or embed namespace, container, node, metric, job, or audit data before that session is verified. Unauthenticated access MUST yield only the sign-in surface.
- **FR-002a**: The session MUST travel in a server-set cookie marked `HttpOnly; Secure; SameSite=Strict`; the session token MUST NOT be exposed to page script, a URL, or browser storage. Every mutating admin route MUST verify the request `Origin` and a CSRF proof before acting, and MUST refuse a request that fails either check before any state changes.
- **FR-003**: Every UI read and write MUST be authorized server-side against the `014` permission vocabulary for the requested scope. Hiding or disabling a control in the browser MUST NOT be the enforcement mechanism; a direct request for an unauthorized action MUST be refused.
- **FR-004**: The console MUST display the signed-in principal, its effective scope (cluster-wide or namespace-scoped), and its session expiry. Sign-out MUST clear the session cookie and leave no browser credential state that a reload could restore.
- **FR-005**: Session expiry or revocation during use MUST cause the next action to be refused as unauthenticated, return the operator to sign-in, and restore the requested destination after successful sign-in. Entered form values MUST NOT be reported as submitted.
- **FR-006**: The console MUST provide one persistent navigation shell covering Overview, Namespaces, Nodes, Monitoring, and links to the cluster map and console owned by `009`. Navigation labels and primary copy MUST use SpaceStorage entities (see **Product goal**); the shell MUST NOT present Elasticsearch product nouns as the operator's primary vocabulary. Every view MUST be addressable by URL such that refresh, bookmark, and browser back/forward reproduce the same view and filters. The shell MUST own `/ui`; `/ui/cluster` and `/ui/console` MUST permanently redirect into the corresponding shell section, which links through to — or embeds, only where `009` already exposes fragments — the `009`-owned views, so existing bookmarks resolve.
- **FR-007**: The Overview MUST present, in one view: overall cluster readiness; counts of ready and not-ready members; namespace and container counts; replica health and replication lag summary; active, failed, and recently completed job counts; and the time of the last successful data load.
- **FR-008**: The Overview MUST surface degraded conditions — not-ready members, degraded or lagging replicas, failed jobs, unavailable controller data — above the fold and MUST name the affected node and namespace/container rather than only showing an aggregate.
- **FR-009**: Every panel MUST show the age of its data and MUST mark itself stale once that age exceeds 30 s. A paused, hidden, or disconnected view MUST NOT be presented as live, and automatic refresh MUST be pausable by the operator.
- **FR-010**: Automatic refresh MUST be bounded to one poll every 10 s, MUST stop while the tab is hidden or the session is invalid, and MUST NOT issue unbounded or overlapping request storms.
- **FR-011**: The Namespaces view MUST list namespaces visible to the principal with, per namespace, its name, container count, and quota summary; it MUST support filtering by name and MUST paginate when the count exceeds one page.
- **FR-011a**: A principal holding cluster administration MUST be able to replace a namespace's quota. The confirmation MUST name the namespace and the proposed quota, invalid values MUST be refused with the `007` surface's reason and applied as no change, and the displayed quota MUST NOT update until the server confirms it.
- **FR-012**: A principal holding cluster administration MUST be able to create a namespace from the UI. Name validation MUST be performed by the owning `007` surface and its refusal reason shown; the UI MUST NOT accept a name the server refuses or silently transform it.
- **FR-013**: A principal holding cluster administration MUST be able to rename a namespace. The result MUST preserve tenant identity as `007` defines it; the UI MUST reflect success or failure and MUST NOT display a renamed state before the server confirms it.
- **FR-014**: Namespace deletion MUST be refused when the namespace still holds containers unless the operator explicitly confirms a cascade. The confirmation MUST name the namespace and state how many containers would be removed. A refused or failed deletion MUST leave the previous view state intact.
- **FR-015**: The Namespace detail view MUST show the namespace record, quota summary, and read-only container inventory, and MUST link to the `009` console for container authoring and data browsing. This feature MUST NOT add container authoring or row browsing of its own.
- **FR-016**: A namespace-scoped principal MUST see only their permitted namespaces in lists, counts, detail views, search results, and any summary. A direct request for a different namespace MUST be refused without disclosing whether it exists or what it contains.
- **FR-017**: The Nodes view MUST list admitted members and pending joins with identity, name, readiness, and controller role, and MUST distinguish ready, not-ready, pending, and unknown states.
- **FR-018**: The Node detail view MUST present the node's state, readiness, worker-thread configuration, buffer usage, and topology labels as reported by the owning admin and membership surfaces. Values not exposed by those surfaces MUST be shown as unavailable; the UI MUST NOT fabricate or default them.
- **FR-019**: The Monitoring views MUST render the metric series and labels defined by `008` for the permitted scope, including a per-namespace view, and MUST NOT rename, re-derive, or synthesize series. An unavailable series MUST be reported as unavailable rather than as zero.
- **FR-020**: Job and migration progress MUST be reviewable in the console, showing job kind, target, status, progress, and failure state, consistent with the owning job surface.
- **FR-020a**: A principal permitted by `010` MUST be able to cancel a running job and resume a paused one from the console, with a confirmation naming the job and its target; the reported state MUST come from the job surface, and a refusal or failure MUST be shown with its reason.
- **FR-021**: The console MUST offer the write operations the CLI admin surface offers — namespace create, rename, and delete (with cascade), configuration reload, quota replace, job cancel and resume, node drain, and cluster stop — plus reading the audit trail for principals holding `AUDIT_READ` or cluster administration. Each action MUST go through its owning surface (`007` tenancy, `001` admin, `010` jobs, `011` membership), MUST require an explicit confirmation naming the target and effect, MUST report the server's outcome, and MUST NOT report success for an unconfirmed, refused, or failed action.
- **FR-021a**: Node drain and cluster stop MUST use the strongest confirmation in the console: the dialog MUST name the specific node, or state that the whole cluster is the target, and MUST state the expected impact — no new tenant connections or placements with replicas moved off before decommission for a drain; every service on the cluster stopping for a cluster stop. It MUST require an acknowledgement beyond a single default button and MUST NOT pre-select the target. For both actions the acceptance suite MUST cover the refusal path (insufficient permission, dismissed dialog, direct request that bypasses the control) and the confirmed path.
- **FR-021b**: The audit view MUST be grounded in a read-only, paginated `audit_list` query on the `014`-owned admin surface, gated by `AUDIT_READ` or `CLUSTER_ADMIN`, and MUST page within `015` row limits rather than requesting an unbounded collection. The console MUST NOT offer retention, export, or audit-configuration changes.
- **FR-022**: A failed, refused, timed-out, or unavailable request MUST be reported in the affected panel with the server's reason where one exists; the remaining panels and navigation MUST stay usable. No view may render a blank page, an infinite spinner, or a fabricated empty state for a failure.
- **FR-023**: All strings originating from cluster data — names, labels, metric values, error messages, audit fields — MUST be rendered as text and MUST NOT be interpreted as markup or script. The console MUST serve a restrictive content security policy and MUST NOT require inline third-party script.
- **FR-024**: The primary flows MUST be operable by keyboard alone, MUST expose semantic landmarks, headings, table headers, and form labels, and MUST NOT convey status by colour alone. Interactive controls MUST have discernible names and a visible focus state.
- **FR-025**: UI requests MUST be attributable in the existing observability model with the console's application label and existing label names, so operators can see the console's own request volume and error rate without new label vocabulary.
- **FR-026**: The console MUST reflect controller state when a control plane is present. When controller data is unavailable, affected views MUST state that the data is unavailable or node-local and MUST NOT present it as cluster-wide truth.

### Non-Functional Requirements

- **NFR-001 (responsiveness)**: An operator opening the Overview on a reference-size cluster MUST see current cluster status within 2 seconds in 95% of loads, and interacting with a table or filter MUST produce a result within 1 second in 95% of interactions.
- **NFR-002 (no data-path impact)**: With ten concurrent operators browsing, data-plane latency MUST stay within 5% of the baseline measured without the console, and no worker-thread stall attributable to UI work may be observed.
- **NFR-003 (bounded resources)**: Every UI request MUST have a bounded response size and row count, consistent with `015` result limits; the console MUST NOT request an unbounded collection, and MUST NOT buffer an entire store to render a page.
- **NFR-004 (failure isolation)**: A single failing or slow dependency MUST degrade only the panels that depend on it. A UI request that exceeds its budget MUST return a timeout result rather than occupying capacity indefinitely.
- **NFR-005 (browser support)**: The console MUST work in the current and previous major versions of the major evergreen browsers at viewport widths from 768 px upward, and at 200% zoom without loss of function or horizontal scrolling on primary flows.
- **NFR-006 (accessibility)**: Primary flows MUST meet WCAG 2.1 AA: keyboard operability, focus visibility, contrast, form labelling, and non-colour status indication.
- **NFR-007 (security headers and secrets)**: Responses MUST carry a restrictive content security policy, a frame-ancestors restriction, and nosniff. The session credential MUST NOT be readable by page script and MUST NOT appear in URLs, page titles, logs, or third-party requests, and MUST be cleared on sign-out. A mutating request whose `Origin` is absent or foreign, or whose CSRF proof is missing or invalid, MUST be refused.
- **NFR-008 (offline/air-gapped operation)**: The console MUST load and function on a cluster with no internet access; no font, script, stylesheet, or icon MAY be fetched from a public origin.
- **NFR-009 (no second toolchain)**: Building, testing, and running the console MUST require only the Rust toolchain already required by the workspace; no additional language runtime or package manager may be mandatory.
- **NFR-010 (observability of the console)**: Console request count, error ratio, and duration MUST be observable per surface using the existing label set, without adding required label names.

### Key Entities

- **UI Session View**: The signed-in principal, effective scope, permission-derived affordances, and expiry, as presented by the shell; carried by a server-set `HttpOnly` cookie that page script never reads.
- **Navigation Section**: A named, addressable area of the console (Overview, Namespaces, Nodes, Monitoring, plus links to `009` views) with its own URL and permission requirement.
- **Cluster Overview Snapshot**: A point-in-time, age-stamped summary of readiness, member counts, namespace/container counts, replica health, job counts, and controller availability.
- **Namespace View**: The `007` namespace record as presented: name, container count, quota summary, and health; carries the tenant identity used for every scoped request.
- **Node View**: An admitted member or pending join with identity, name, readiness, controller role; expanded by a Node Detail carrying state, worker-thread configuration, buffer usage, and topology labels.
- **Monitoring Panel**: A bounded view over `008` series for a scope, with availability state and data age; never a redefinition of a series.
- **Job Progress View**: Kind, target, status, progress, and failure state of a migration, transform, backup, or restore job as the owning surface reports it.
- **Guarded Action**: A permitted operational action presented with target, expected effect, confirmation strength (with the strongest form reserved for node drain and cluster stop), and outcome; audited by the owning surface.
- **Refusal/Error Presentation**: The operator-visible form of an authentication, authorization, validation, timeout, or availability failure, including the server's reason when available.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: An operator who signs in cold identifies a not-ready node and a lagging replica in under 60 seconds in 100% of degraded-cluster fixtures, without using the CLI.
- **SC-002**: A cluster administrator creates a namespace in under 30 seconds and no more than three interactions from the Overview, and the new namespace is visible without a manual reload, in 100% of runs.
- **SC-003**: 100% of cluster-scoped controls are absent or disabled for namespace-scoped principals, and 100% of directly attempted cluster-scoped actions by such principals are refused server-side.
- **SC-004**: 0 records of namespace, container, node, metric, job, or audit data are rendered before session verification across 100% of unauthenticated probes.
- **SC-005**: On a reference cluster of three nodes, 50 namespaces, 500 containers, and 5,000 shards, 95% of Overview loads show current cluster status within 2 seconds; 100% of panels older than 30 s are marked stale; no panel polls more often than once every 10 s; and 0 polls are issued while the tab is hidden.
- **SC-006**: With ten concurrent operators browsing for ten minutes, data-plane latency stays within 5% of the no-console baseline and 0 worker-thread stalls are attributable to UI work.
- **SC-007**: 100% of destructive actions present a confirmation naming the target; 100% of node drain and cluster stop confirmations name the specific node or the whole cluster and state the impact; and 0 destructive actions complete without an explicit confirmation in the suite.
- **SC-008**: 100% of primary flows — sign in, Overview, create namespace, open node detail — are completable by keyboard alone, and automated accessibility checks report 0 critical violations on those flows.
- **SC-009**: 100% of injected endpoint failures (server error, timeout, refusal, unavailable dependency) render an explanatory panel while the rest of the page remains usable; 0 blank pages and 0 unlabelled infinite spinners.
- **SC-010**: 100% of rendered metric series and labels match the `008` definitions; 0 synthesized or renamed series appear in the suite.
- **SC-011**: On a cluster with no internet access, 100% of console pages load and primary flows complete, with 0 requests to public origins.
- **SC-012**: 100% of mutations reflected in the UI — namespace rename and delete, quota replace, job cancel and resume, node drain, cluster stop — match the owning surface's final state; 0 views show a mutation as applied when the server refused or failed it.
- **SC-013**: 0 session credentials are readable through page script or browser storage in 100% of probes; 100% of mutating requests with a missing or foreign `Origin`, or a missing or invalid CSRF proof, are refused before any state change.
- **SC-014**: 100% of audit views are served from `audit_list`, paginate within `015` row limits, and are refused without `AUDIT_READ` or cluster administration; 0 audit retention, export, or configuration controls exist in the console.

## Assumptions

- The console is an administrative surface for operators and tenant administrators; it is not a public marketing site and not an end-user application console.
- **Q1 (resolved, Session 2026-10-31)**: the console has full write parity with the CLI admin surface — namespace create/rename/delete with cascade, configuration reload, quota replace, job cancel and resume, node drain, and cluster stop; drain and cluster stop use the strongest confirmation described in FR-021a.
- **Q2 (resolved, Session 2026-10-31)**: bounded polling every 10 s, data marked stale after 30 s, a visible age on every panel, an operator pause control, and refresh suspended while the tab is hidden; no streaming transport.
- **Q3 (resolved, Session 2026-10-31)**: the session is a server-set `HttpOnly; Secure; SameSite=Strict` cookie with `Origin`/CSRF verification on mutating routes and no script-readable token; `014`'s fixed 12 h lifetime with no idle timeout is inherited unchanged.
- **Q4–Q9 (not asked within the five-question limit; recorded assumptions)**: one accessible light theme with no branding assets (Q4); audit is read-only and permission-gated, served by `audit_list`, with no audit mutation or retention controls (Q5); the shell owns `/ui` and its redirects while `009` keeps the map and console content (Q6); no container authoring or row/data browsing here, container inventory is read-only with links into `009` (Q7); WCAG 2.1 AA for primary flows (Q8); English-only copy (Q9).
- **Q10 (resolved, Session 2026-11-22)**: Cerebro and Kibana are a **capability-class goal** for the combined admin web surface, mapped onto SpaceStorage entities (namespace, container, shard, replica, …); not a pixel or noun clone of those Elasticsearch products.
- The UI is an operator tool: it may assume a stable network to the cluster, but must recover gracefully when a node is unreachable or the session is invalid.
- Reference cluster for performance figures: three nodes, 50 namespaces, 500 containers, 5,000 shards, one active migration job.
- Existing admin and tenancy surfaces already expose the data and writes this console presents; where a field is absent the console shows it as unavailable rather than extending the owning surface — with exactly three recorded exceptions, each drawn into this feature's plan scope and none redefining an owner's semantics: (i) the read-only paginated `audit_list` query on the `014` admin surface (FR-021b); (ii) the `014` session change from a page-held bearer token to an `HttpOnly` cookie with `Origin`/CSRF verification on mutating routes (FR-002a); (iii) an admin-path entry point for the drain and undrain operations `011` membership already defines, so the console can invoke them. No other surface is extended.

## Dependencies

- **`014` authz and session**: sign-in, session lifecycle, permission vocabulary, audit. The console cannot ship without a session surface. **Cross-feature dependency**: `014` owns session issuance and today returns a 12 h bearer token that the page stores in `sessionStorage`. This feature's plan scope includes the `014` surface change — issue the session as an `HttpOnly; Secure; SameSite=Strict` cookie and verify `Origin`/CSRF on mutating admin routes — plus the read-only paginated `audit_list` query gated by `AUDIT_READ`/`CLUSTER_ADMIN`. Session lifetime, vocabulary, and audit records stay `014`'s; `014`'s spec and plan must be reconciled with both changes before the console ships.
- **`001` admin HTTP**: status, configuration, threads, buffers, reload, cluster stop.
- **`007` tenancy**: namespace list/create/rename/delete, quota replace, quotas, name rules.
- **`006`/`011` control plane and membership**: controllers, Raft status, membership roster, pending joins, and the drain/undrain membership operations. `011` owns their semantics; exposing them on the admin path is a prerequisite this feature's plan records.
- **`008` observability**: metric names, labels, and the namespace-scoped metric surface.
- **`010` migration and jobs**: job listing, status, progress, cancel, and resume.
- **`004`/`012` placement and topology**: replica health, lag, and the label ladder shown on node detail.
- **`009` cluster map and console**: linked, not reimplemented; the map graphic and browse console remain its responsibility.
- **`015` limits**: response size and row bounds that the UI must respect.

## Out of Scope

- Kafka and syslog **ingest** — owned by `009`.
- The Cerebro-class cluster-map graphic and the Kibana-class configure/browse console — owned by `009`; this feature links to them. Their product goal (Cerebro/Kibana-class capability over SpaceStorage entities) is stated in **Product goal** above; this feature does not re-implement those views.
- Cloning Elasticsearch UI chrome, icons, or product vocabulary (index, document, ILM, and similar) as the console's primary language.
- A second query engine, a second configuration store, a second schema authority, or a UI-private privilege model.
- Metric series definitions, renames, or a new label vocabulary — owned by `008`.
- Bulk or scheduled destructive automation — one confirmed drain or one confirmed cluster stop at a time; no fleet-wide scripting, no multi-node drain batch, no scheduled shutdown.
- Node decommission and replace workflows — drain and undrain are the console actions; decommission and replace stay procedural/CLI.
- Audit retention, audit export, and audit configuration — the console reads audit only.
- Custom role authoring and permission editing in the UI — `007`/`014` own the model; this feature only enforces and displays it.
- Backup/restore authoring beyond reviewing job progress.
- Billing/invoicing views, legal hold and erase workflows, CDC views.
- Dark theme, branding, localisation/i18n, and mobile phone layouts below tablet width.
- A separate frontend service, a public CDN, or a mandatory Node.js toolchain (constitutionally prohibited).

## Grounding: Existing Surfaces Consumed (baseline, not new contract)

Current working tree, for planning context only. These endpoints exist today; this feature consumes them and does not redefine them, except for the two changes called out below.

- `GET /v1/status`, `/v1/config`, `/v1/threads`, `/v1/buffers`, `POST /v1/reload`, `POST /v1/stop`
- `POST /v1/admin` with operations for `status`, `controllers`, `raft_status`, `leases`, `namespace_list`, `namespace_create`, `namespace_rename`, `namespace_delete`, `namespace_describe`, `quota_replace`, `role_list`, `role_binding_list`, `membership`, `join_token_mint`
- `GET /metrics`, `GET /metrics/namespaces/{name}`
- `GET /v1/jobs`, `GET /v1/jobs/{id}`, `POST /v1/jobs/{id}/cancel`, `/resume`
- `POST /v1/auth/login` (session token exchange today; this feature's plan replaces the returned page-held token with an `HttpOnly` session cookie and adds `Origin`/CSRF checks on mutating routes)
- `GET /v1/cluster/map`, `POST /v1/console/query`, `/v1/console/config`, `/v1/console/containers` (owned by `009`)
- `GET /v1/health/live`, `/v1/health/ready`

Not present today, and added by this feature's plan scope rather than consumed as baseline: a read-only paginated `audit_list` operation on the `014` admin surface, and an admin-path entry point for `011`'s existing drain/undrain membership operations.

Existing UI baseline in `crates/admin-ui`: `/ui` → redirect, `/ui/cluster`, `/ui/console`, `/ui/assets/{name}`; pages built server-side, assets served from the binary; per-principal map/console authorization derived from `014`-shaped roles. Assets today read the session token from `sessionStorage`; that token moves into the `HttpOnly` cookie described above. The modern console replaces the shell around these views, not their authorization semantics, and `/ui/cluster` and `/ui/console` become permanent redirects into it.
