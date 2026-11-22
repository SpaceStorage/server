# Contract: Session cookie, Origin, and CSRF

**Feature**: `018-admin-ui-modernization` | Owner of the session record: `014` (`crates/authz`) | HTTP: `crates/node` admin-http

This contract is the `014` delta drawn into `018`. Lifetime, verb vocabulary, and `AuditEntry` layout stay `014`. The session secret is no longer returned to page script.

## Cookie

Name: `ss_session`.

Value: hex of the 32-byte secret `SessionTokenStore::issue` already creates. The store keeps SHA-256 only.

Attributes on every set, including logout clear:

```http
Set-Cookie: ss_session=<hex>; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=43200
```

Logout uses an empty value and `Max-Age=0`, and `SessionTokenStore::revoke_hash` drops the record.

`Secure` is unconditional. Loopback browsers accept it on `http://localhost` and `http://127.0.0.1`. A non-loopback plaintext listener will not retain the cookie in a browser; operators use TLS there.

## Login

`POST /v1/auth/login`

Request body, unchanged: `{"token":"<admin token file contents>"}` or `Authorization: Bearer` of that admin token. This bearer is the **admin token file**, not a session.

Success **200**:

```json
{
  "expires_in": 43200,
  "expires_at": "2026-11-23T06:47:00Z",
  "principal": "admin",
  "principal_id": "018f2c6e-7b1a-7c3d-8e4f-1a2b3c4d5e6f",
  "scope": "cluster",
  "namespaces": [],
  "csrf_token": "<64 hex chars>",
  "affordances": {
    "namespace_write": true,
    "quota_replace": true,
    "config_reload": true,
    "cluster_stop": true,
    "node_drain": true,
    "job_control": true,
    "audit_read": true,
    "metrics_read": true,
    "map": true,
    "browse": true
  }
}
```

The body has no `token` and no `token_type`. `Set-Cookie` is as above.

Failure **401**:

```json
{ "code": "unauthorized", "message": "invalid admin token", "details": {} }
```

No `Set-Cookie` on failure.

`csrf_token` is 32 random bytes, hex-encoded, stored on the session record beside the hash. It is minted at login and not rotated on GET.

## Session probe

`GET /v1/auth/session`

- Cookie resolves and is unexpired → **200** with the same JSON shape as login (`expires_in` is the remainder).
- Missing, unknown, or expired cookie → **401**

```json
{ "code": "session_invalid", "message": "session expired or unknown", "details": {} }
```

This code is not used for a wrong admin token at login.

The probe returns no namespace payloads, node rows, or metrics.

## Logout

`POST /v1/auth/logout`

Cookie-authenticated mutation (rules below). Success **204**, `Set-Cookie` clears `ss_session`, hash revoked. A reload does not restore the session.

## Who must pass Origin and CSRF

Applied to every state-changing admin-http route when the caller is the browser session. Covered routes include:

- `POST /v1/auth/logout`
- `POST /v1/admin`
- `POST /v1/reload`
- `POST /v1/stop`
- `POST /v1/jobs`, `POST /v1/jobs/{id}/cancel`, `POST /v1/jobs/{id}/resume`
- `POST /v1/console/query`, `POST /v1/console/config`, `POST /v1/console/containers`

GET, HEAD, and OPTIONS do not require the CSRF header.

### Cookie session

All three, checked before the handler runs:

1. `Cookie` contains a resolvable `ss_session`.
2. `Origin` is present. Its host and port equal the request `Host` header. Scheme is `https`, or `http` only for host `localhost` or `127.0.0.1`. `X-Forwarded-Host` and `Referer` are not consulted.
3. `X-SpaceStorage-CSRF` equals the session's `csrf_token` in constant time.

Failure **403**, body:

```json
{ "code": "csrf_rejected", "message": "origin or csrf check failed", "details": {} }
```

The handler does not run. No namespace, membership, job, or audit mutation is applied.

A request that omits the cookie and omits an accepted admin-token bearer is **401** `session_invalid` on these routes (not a CSRF error).

### Admin token file (CLI)

`Authorization: Bearer <contents of admin.token_file>` is accepted as today, without `Origin` and without `X-SpaceStorage-CSRF`. It is the cluster-admin credential `001` already uses. It is not a browser-ambient cookie.

### Session secret as bearer

`Authorization: Bearer <session hex>` → **401** `session_invalid`. The console must not keep a copy in `sessionStorage`, a URL, or a script-visible cookie.

## `next` after login

The login document may be opened as `/ui/overview?next=/ui/namespaces/acme`. `next` is honored only when it is a single relative path starting with `/ui/`, with no scheme, no `//`, and no backslash. Otherwise the shell loads `/ui/overview`.

## Tests this contract requires

- Login `Set-Cookie` contains `HttpOnly`, `Secure`, and `SameSite=Strict`. JSON has `csrf_token` and no `token`.
- `document.cookie` and `sessionStorage` in the shell script never receive the session hex. A conformance probe of the HTML and of `shell.js` finds no `sessionStorage` write of a token.
- `POST /v1/admin` with the cookie, no `Origin` → 403 `csrf_rejected`, and a following namespace list does not contain a name the refused call tried to create.
- Same call with `Origin: https://evil.example` → 403 `csrf_rejected`.
- Same call with a wrong `X-SpaceStorage-CSRF` → 403 `csrf_rejected`.
- Same call with matching `Origin` and CSRF → the owning operation runs.
- CLI bearer of the admin token file still executes `NamespaceList` without a CSRF header.
- Logout then `GET /v1/auth/session` → 401 `session_invalid`.
