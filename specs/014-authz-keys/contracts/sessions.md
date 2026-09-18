# Contract: Sessions and later-request checks

**Feature**: `014-authz-keys` | Crate: `authz` | Spec: FR-016 | Research R8

## At AUTH

Copy `(principal_id, namespace_id?, credential_generation)` onto the connection. Unverified connections MUST NOT read or write tenant data.

## Later request

1. Load `PrincipalRecord` by id.
2. `enabled == false` → `PrincipalDisabled`.
3. `credential_generation` ≠ session copy → `AuthGenerationMismatch`.
4. `authorize` against **current** `007` bindings (and first-binary implicit tenant grant).

A request already in flight MAY finish. The server MUST NOT be required to close the TCP connection.

## Login rename

Does not bump `credential_generation`. Later requests still run as that principal id.

## Admin bearer

`AuthLogin` → opaque 32-byte token; store SHA-256 + principal id + generation + expiry (default 12 h) in the cluster log. Presentation: `Authorization: Bearer` on `admin-http`, or the `admin` TCP auth frame. Password change invalidates via generation. First binary MAY send the password on every admin call instead.
