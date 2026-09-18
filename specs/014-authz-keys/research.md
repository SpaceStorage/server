# Research: Authentication, Authorization, Encryption in Transit, Keys, and Audit

**Feature**: `014-authz-keys` | **Date**: 2026-09-18

Each item resolves a Technical Context unknown or a technology choice. Format: Decision / Rationale / Alternatives considered. Inputs: [spec.md](spec.md) (clarify 2026-09-15/16/18), constitution 1.3.0, intent `14`, sibling plans `001` (tls/plaintext, interim admin token), `002` (Authenticator, users_file), `003` (KeyAuthority, keyring), `007` (role names/bindings), `008` (optional audit export), `011` (bootstrap, join secret), `012` (replication role), `013` (WAL data keys), `016` (slices).

## R1. One crate for principals, verbs, sessions, audit

- **Decision**: Add `crates/authz` (`spacestorage-authz`). It owns `PrincipalRecord`, SCRAM verifiers, the closed verb set + implication, session generation checks, bootstrap of the first `CLUSTER_ADMIN`, and `AuditEntry` append/read. Cluster-log bodies are typed here; `controlplane` appends them to the **cluster** group. `007` `tenancy` keeps `Role` / `RoleBinding` rows and calls `authz::authorize`. Envelope unwrap stays in `crates/crypto` (R5).
- **Rationale**: Same split as `006` vs `placement` and `007` vs Raft: tests of SCRAM/implication must not boot Raft. Audit vocabulary is this feature, not `08`.
- **Alternatives considered**: Grow `tenancy` (rejected: quotas vs PBKDF2); `crates/audit` separate (rejected: one privileged-action list, extra crate for a log); put principals in namespace Raft (rejected: cluster-wide identity, `007` Q2).

## R2. Bootstrap config creates the first CLUSTER_ADMIN

- **Decision**: `cluster { bootstrap; admin_login L; admin_password_file P; }` is required on first-node bootstrap (`011`). Apply creates one `PrincipalRecord` (login `L`, SCRAM verifier from the password file) and a `RoleBinding` to builtin `admin`. Missing login or unreadable file → startup `BootstrapAdminRequired`. If `identity/cluster.json` already exists, bootstrap flags are ignored for minting (no second admin). Join secret (`cluster.token_file`) NEVER maps to this principal.
- **Rationale**: Spec FR-015 / clarify 2026-09-18 Q2.
- **Alternatives considered**: Loopback-open until first user (rejected: Q2 option B); join secret as admin (rejected: Q2 option C; `011` stolen-secret rule); `001` static `admin.token_file` kept forever (rejected: two admin planes).

## R3. Replace interim admin token and users_file

- **Decision**: After this feature, `admin` / `admin-http` authenticate the principal store (password → optional bearer, R8). `001` `admin { token_file }` is **removed** as a CLUSTER_ADMIN source (`AdminTokenRemoved`). `002` `auth { users_file }` is **removed**; handlers call `authz::Authenticator`. First-binary profile that still has those directives → validate error naming the replacement.
- **Rationale**: Closes `001` XIII and `002` XIII interim deviations. One store.
- **Alternatives considered**: Keep both and dual-read (rejected: two passwords); alias token_file to bootstrap password (confused with `011` join secret).

## R4. SCRAM verifier is the only stored secret

- **Decision**: Store SaltedPassword / ServerKey / StoredKey + salt + iteration count (SCRAM-SHA-256). Never store plaintext. PostgreSQL uses `pgwire` SCRAM. Redis AUTH presents a password; the server runs the same SCRAM verify locally (does not speak SCRAM on the Redis wire). Iteration count: config `auth { scram_iterations N; }` default **16384**, test override 4096. Password change increments `credential_generation` and writes a new verifier.
- **Rationale**: Spec FR-001; `002` already selected SCRAM for PG.
- **Alternatives considered**: Plaintext users_file (rejected: interim only); bcrypt (not what PG clients speak).

## R5. Envelope KeyAuthority replaces keyring_file

- **Decision**: `keys { master_key_file P; }` (32 random bytes, `0600`). Per-namespace KEK (32 bytes) is generated at namespace create, wrapped with AES-256-GCM under the master, stored in the cluster log (`KekRecord`). A container `key_ref` names a **data key** (32 bytes) wrapped under that namespace KEK; wrapped blob + id live with the container definition (`003`). `EnvelopeAuthority::resolve` unwraps master→KEK→data key into `Zeroizing<[u8;32]>`; `003` still HKDF-SHA-256 salts with `ContainerId` for per-block keys (existing `encryption.md`). Nodes **cache** unwrapped data keys only for containers they host (policy). Cryptographically, every node with the master file can unwrap any KEK — documented; threat model is operator-runs-nodes, not enclave.
- **Rotate master**: new master file (or in-place rewrite after backup copy), rewrap all `KekRecord`s, data keys unchanged, 0 table rewrites (SC-005).
- **Rotate data key**: new wrapped data key version on the container; old versions retained until `010` rewrite.
- **Restore**: `--key REF` or `--master-key-file` on restore (`013`); missing → error names the reference (SC-004/007).
- **Rationale**: Spec FR-008/009; `003` KeyAuthority seam; clarify 2026-09-15 Q2/Q3.
- **Alternatives considered**: Keep keyring_file as master (wrong shape: no KEK layer); KMS first (rejected: `016`); HKDF-only from master with no stored KEK (namespace compromise would not isolate).

## R6. TLS is already 001; this feature adds optional mTLS

- **Decision**: Do not re-specify `tls { }` / `plaintext;`. Consume `001` FR-027–030. Optional `tls { client_ca P; }` (reserved in `001`): require a client cert; map **CN** (or first DNS SAN if CN empty) to principal **login**. First binary: mTLS is **additional** (password still required) unless `tls { mtls_replace_password; }` on that entrypoint. No connect-option namespace selector on Redis/S3/WebDAV/ES.
- **Rationale**: Spec FR-005; `001` already owns omitted-transport startup error (SC-003).
- **Alternatives considered**: Duplicate TLS grammar here (drift); mTLS-only as default (breaks stock `psql`/`redis-cli` smoke).

## R7. Audit source of truth is the cluster log

- **Decision**: `AuditAppend` is a cluster-log body. Fields: principal id, optional login-at-event, action, target, HLC (`012`), optional namespace id. First binary MUST append: `auth.fail`, `auth.ok` (admin/join-related), `membership.join|leave|replace|token`, `key.bind|rotate_master|rotate_data`, `principal.rename`, `namespace.rename`. `CLUSTER_ADMIN` reads via admin (`AuditList`). `08` audit **channel** is optional export of these entries (off by default, slice 9); it is not the store. Tenant `AUDIT_READ` is slice 7.
- **Rationale**: Clarify deferred SoT; join tokens must be auditable in the first binary (`011`) before `08` sinks exist.
- **Alternatives considered**: Local files (lost on disk replace); `08` only (not first binary); namespace Raft (cluster events have no tenant).

## R8. Session re-check via credential_generation; optional bearer

- **Decision**: `PrincipalRecord.credential_generation: u64` increments on password change. Session stores `(principal_id, namespace_id, generation)` at AUTH. Each later request: principal enabled; `generation` match; current grants allow the verb. Mismatch → protocol auth/authorization error; TCP NEED NOT drop. In-flight MAY finish. Login rename does not bump generation. Disable sets `enabled=false` (same re-check). Grant reduction (slice 7) uses current role bindings, not a session snapshot.
- **Admin bearer**: `AuthLogin` returns a random 32-byte token; server stores **SHA-256** of it in the cluster log (`SessionToken`) with TTL default **12 h**, bound to principal id + generation. Password change invalidates via generation. First binary MAY use password on every admin call instead.
- **Rationale**: Spec FR-016 / clarify 2026-09-18 Q5; spec “bearer token or the same store”.
- **Alternatives considered**: Forced disconnect (harder, not required); keep grants snapshot (stale WRITE after strip).

## R9. CLUSTER_ADMIN implies every verb; builtins immutable

- **Decision**: `Verb` is a closed bitmask. `CLUSTER_ADMIN` bit ⇒ `Authorizer` returns allow for any other verb and any namespace. Builtin role `admin` is exactly `{CLUSTER_ADMIN}`; `replication` is `{REPLICATE}`. `RolePut`/`RoleDelete` on those names → `BuiltinRoleImmutable`. Custom name `admin`/`replication` → `RoleNameReserved`. Slice 7 custom role that includes `CLUSTER_ADMIN` gets the same implication.
- **Rationale**: Spec FR-003 / clarify 2026-09-18 Q4.
- **Alternatives considered**: Explicit full set on `admin` (equivalent, noisier); editable builtins (rejected).

## R10. Unbound admin on credential-bound protocols

- **Decision**: `Authenticator` for Redis/S3/WebDAV/ES: if the principal has no `RoleBinding.namespace_id`, refuse at AUTH (`UnboundCredential`), including builtin `admin`. PG/Cassandra/ClickHouse/admin: `CLUSTER_ADMIN` MAY select any existing namespace by native name. Admin data-plane on Redis uses a **bound** principal.
- **Rationale**: Spec FR-002 / clarify 2026-09-18 Q1; `002` FR-009a.
- **Alternatives considered**: Admin SELECT-as-namespace (rejected: Redis SELECT is no-op in `016`); wildcard session (breaks one-namespace-per-session).

## R11. Replication is not a tenant principal

- **Decision**: No login named `replication` in the tenant store. `012` continues to authenticate peers with join secret + builtin `replication` role (`007` binding to node identity). Presenting that identity on a tenant handler → `ReplicationNotTenant`.
- **Rationale**: Spec FR-002; `007`/`012` already stated this.
- **Alternatives considered**: Password principal for nodes (duplicates join secret).

## R12. Who may create, rename, reset, disable principals

- **Decision**: **First binary**: `CLUSTER_ADMIN` creates/disables/deletes/resets password/renames any principal; a principal MAY change **own** password and **own** login. **Slice 7**: `NAMESPACE_ADMIN` MAY create/rename/reset/disable principals **bound to their namespace** only. Bindings always use principal id (`007`).
- **Rationale**: Spec FR-014; `CLUSTER_ADMIN` implication (R9); `NAMESPACE_ADMIN` “principals inside one namespace”.
- **Alternatives considered**: Only CLUSTER_ADMIN forever (too weak for tenant self-service in slice 7).

## R13. READ/WRITE granularity (slice 7)

- **Decision**: Custom grants are **namespace-wide** for a verb. Optional `container_ids: Vec<Uuid>` on the binding narrows READ/WRITE/CONFIGURE to those containers; empty = all containers in the namespace. CREATE/DROP stay namespace-wide. First binary has no custom grants.
- **Rationale**: Spec “named containers or all containers”; simplest slice-7 shape.
- **Alternatives considered**: Per-type only (`007` access policies already cover type deny); delay all container lists (would ignore the spec’s “named” clause).

## R14. Failed-auth rate limits

- **Decision**: First binary: count `spacestorage_auth_attempts_total{result=fail}` only. No lockout, no IP ban (hostile-tenant isolation is namespace, not scanner defense). MAY add a documented delay later without a vocabulary change.
- **Rationale**: Clarify deferred; not required for SC-006.
- **Alternatives considered**: Lock after N fails (false-positive on shared NAT; not specified).

## R15. Master key file missing

- **Decision**: If any container declares encryption, or `keys { master_key_file }` is set, the file MUST exist, be 32 bytes, mode `0600` (Unix). Bootstrap of an encrypting cluster SHOULD generate the file if missing **only when** `keys { create_master_if_absent; }` is set (laptop); production omits that flag and fails `MasterKeyRequired`. Backup = copy the file; documented in quickstart.
- **Rationale**: Spec FR-009 backupable; stolen disk of encrypted data remains ciphertext.
- **Alternatives considered**: Always autogen (operators would not back up); embed in cluster log (then stolen Raft snapshot is the key).

## R16. Slice flags

- **Decision**: `release-profile` first binary compiles `authz` without custom RolePut. `authz-custom` (slice 7) enables custom roles, container-scoped grants, tenant `AUDIT_READ`, `NAMESPACE_ADMIN` principal/key ops. Data-key rewrite remains `010` (`migrate`).
- **Rationale**: Spec FR-013; `016` slice 7 “full authz vocabulary”.
- **Alternatives considered**: Entire vocabulary in first binary (rejected: clarify Q3 option C).

## R17. First-binary tenant data-plane grant

- **Decision**: Until slice 7 custom roles exist, a namespace-bound principal that is not `admin` or `replication` has an implicit `{READ, WRITE, CREATE, DROP, CONFIGURE}` on **that namespace only**. Not a stored custom role. Slice 7 replaces it with explicit custom sets (empty set = authenticate only). Redis smoke cannot use unbound `admin` (R10), so a bound tenant login must be able to SET/GET in the first binary.
- **Rationale**: `016` PG/Redis smoke; `002` credential-bound Redis; clarify Q1.
- **Alternatives considered**: Force all first-binary data-plane through PG as CLUSTER_ADMIN (rejected: Redis AUTH is a first-binary MUST); ship custom roles in slices 1–5 (rejected: R16).
