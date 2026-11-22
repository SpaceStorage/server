# Quickstart: Modern Admin Web Console

**Feature**: `018-admin-ui-modernization` | **Date**: 2026-11-22

Validation guide for implementers. Contracts and fields live in [data-model.md](data-model.md) and [contracts/](contracts/). This file does not repeat them and does not contain implementation code.

Run from the Cargo workspace root (`server/`).

## Prerequisites

- Rust toolchain matching workspace `rust-version` (1.88, edition 2024). No Node.js and no network fetch of scripts or fonts.
- A node config with an `admin-http` entrypoint and an `admin.token_file`. Loopback HTTP is enough for cookie tests: browsers store the `Secure` cookie on `localhost` / `127.0.0.1`.
- Three-node conformance harness already used by `011` / `009` when a scenario says "three nodes". Single-node scenarios use one process.

Build and test the crates this feature touches:

```bash
cargo test -p spacestorage-admin-ui -p spacestorage-authz -p spacestorage-admin-proto -p spacestorage-membership
cargo test -p spacestorage-conformance --test ui_session_auth
```

Add the new conformance test module name beside `ui_session_auth` once it exists (cookie, CSRF, shell redirects, `audit_list`, drain). The merge gate for the workspace remains:

```bash
cargo test -p spacestorage-conformance
```

## 1. Unauthenticated shell

1. `GET /ui/overview` with no cookie.
2. Expect **200** HTML, a login form, and no namespace, node, metric, or container text.
3. Expect **no** request log line for `/v1/ui/*` from that page load. The HTML must not reference those URLs as immediate fetches before login. Confirm by reading the document: data URLs appear only inside the script path that runs after `GET /v1/auth/session` returns 200.
4. Response includes the CSP from [contracts/shell-http.md](contracts/shell-http.md) and `nosniff`.

## 2. Login cookie

1. `POST /v1/auth/login` with the admin token file.
2. Expect **200** JSON with `csrf_token`, `principal`, `scope`, `expires_at`, and **no** `token` field.
3. `Set-Cookie` contains `ss_session`, `HttpOnly`, `Secure`, and `SameSite=Strict`.
4. `GET /v1/auth/session` with that cookie returns **200** and the same principal.
5. Open the shell in a browser on loopback. DevTools Application → Cookies shows `ss_session` as HttpOnly. Console `document.cookie` does not contain the secret. `sessionStorage` has no session key.

## 3. CSRF refusal and success

1. With the cookie, `POST /v1/admin` `{"op":"namespace_create","name":"csrf-probe"}` and no `Origin`.
2. Expect **403** `csrf_rejected`. `GET /v1/ui/namespaces` does not list `csrf-probe`.
3. Repeat with `Origin: https://evil.example` and a valid CSRF header. Same refusal, same absent name.
4. Repeat with `Origin` matching the request `Host` (loopback `http` allowed) and `X-SpaceStorage-CSRF` equal to `csrf_token`.
5. Expect the `007` result (created, or a validation refusal whose `message` is shown and whose failure does not insert a row).

CLI check: the same create with `Authorization: Bearer` of the admin token file and no CSRF header still reaches `007`.

## 4. Redirects

1. `GET /ui` → **308** `Location: /ui/overview`.
2. `GET /ui/cluster?namespace=acme` → **308** `/ui/map?namespace=acme`.
3. `GET /ui/console` → **308** `/ui/browse`.

## 5. Overview on a degraded cluster

Reference shape: [contracts/fixtures/overview-degraded.json](contracts/fixtures/overview-degraded.json).

1. Three nodes. Mark one member not ready. Induce lag on one replica of `acme/orders` (the `009` / `004` harness).
2. Sign in as cluster admin. Open `/ui/overview`.
3. Within 2 s the page names `n3` (or the fixture's not-ready node) and `acme/orders` without scrolling.
4. Each panel shows an age. Pause refresh. After 30 s the panel text includes "stale".
5. Hide the tab for more than 30 s. On return the stale label is visible until the next response, and no poll was issued while hidden (assert with a request counter in the test, or the server access log).

Single-node: stop the membership roster path (or use a one-node lab). Overview lists the local node and does not error.

## 6. Namespace lifecycle

1. As cluster admin, open Namespaces on an empty cluster. Empty state offers create, not an error string.
2. Create `acme`. The list includes it without a manual reload.
3. Submit a name `007` rejects. The `message` appears beside the field. The list is unchanged.
4. Add a container through `/ui/browse` (the `009` console). Delete `acme` without cascade. The refusal states that containers remain.
5. Confirm cascade delete per **§6.5** below. The namespace disappears only after **200**.
6. Sign in as a namespace admin of `acme`. The list is only `acme`. `GET /v1/ui/namespaces/other` is **403** and the body has no foreign containers.

### 6.5 Cascade delete confirmation

Confirm cascade by acknowledging a dialog that **names the namespace and states the container count** (word as **at least N** when `container_count_complete` is false). This is **normal** confirmation strength — the operator does **not** type the namespace to proceed (unlike strong drain or cluster-stop confirmations). On this small catalog the count in the dialog matches `container_count` exactly.

Page-size check: request `limit=101`. Expect **400** `limit_exceeded`. `limit=50` returns at most 50 names and a `next_cursor` when more exist. Fixture: [contracts/fixtures/namespace-page.json](contracts/fixtures/namespace-page.json).

## 7. Nodes and monitoring

1. Cluster admin, Nodes: members plus one pending join, with readiness text `ready`, `not_ready`, or `pending`.
2. Open a local node. Threads, buffers, and labels match `GET /v1/threads` and `GET /v1/buffers` for that process, or the field says unavailable.
3. Namespace admin: the node list is only hosts of that namespace's containers or shards.
4. Monitoring for `acme` shows series names identical to the `008` registry for that namespace. Stop the metric source (or request a family that is not registered) and confirm the panel says unavailable rather than `0`.

## 8. Audit

1. Principal without `AUDIT_READ` and without `CLUSTER_ADMIN`: `/ui/audit` shows a refusal, and `GET /v1/ui/audit` is **403**.
2. Cluster admin: entries match [contracts/fixtures/audit-page.json](contracts/fixtures/audit-page.json) keys (`principal`, `action`, `target`, `time`). The page has no export or retention control.
3. A second page, when `next_cursor` is set, does not repeat the first page's ids.

## 9. Drain, undrain, and cluster stop

1. Open drain on a named member. The dialog contains that node name and the impact sentence (no new tenant connections or placements; replicas move off before decommission). The type-in field is empty.
2. Dismiss. No `POST /v1/admin` with `op: drain`.
3. Type the node name and confirm. Membership shows `draining`. The process is still up.
4. Undrain with a normal confirmation. Status returns to `ready`.
5. As namespace admin, the drain control is absent, and a direct `op: drain` is **403** with status unchanged.
6. Cluster stop dialog names the cluster and requires typing `cluster`. Dismiss sends no `POST /v1/stop`.

## 10. Keyboard and air gap

1. With the network blocked to public origins, load `/ui/overview`, sign in, create a namespace, and open a node. The browser issues zero requests whose host is not the admin listener. No font or script URL is absolute to another origin.
2. Complete sign-in, Overview, namespace create, and node detail with the keyboard only. Focus is visible. Status text does not rely on color alone.

## Done when

- Sections 1–4 pass in `cargo test` (cookie, CSRF, redirects, unauthenticated HTML).
- Sections 5–9 pass in `crates/conformance` against the lab harness, or are listed as still failing with the assertion that failed.
- Section 10 is recorded once by a human on loopback (keyboard and offline). The structural HTML checks (landmarks, labels, empty acknowledgement `value`) are in `cargo test` so they do not wait on that pass.
