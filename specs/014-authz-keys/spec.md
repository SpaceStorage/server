# Feature Specification: Authentication, Authorization, Encryption in Transit, Keys, and Audit

**Feature Branch**: `014-authz-keys`

**Created**: 2026-09-15

**Updated**: 2026-09-19

**Status**: Draft

**Input**: User description: "Read .specify/intent/14-authz-keys.md and specify this feature." — protocol-native auth mapped to a principal store; one-namespace binding for non-admin principals; closed permission vocabulary; every entrypoint `tls` or `plaintext;`; envelope encryption with cluster master key wrapping namespace KEKs in controller storage; encryption at rest opt-in; audit log.

## Clarifications

### Session 2026-09-15

- Q: Is TLS globally mandatory? → A: No. Every entrypoint MUST declare `tls { ... }` or `plaintext;`. Omitted is a startup error. Plaintext on a non-loopback tenant port is an operator-chosen exposure.
- Q: Where do KEKs live in the first binary? → A: Cluster master-key file wraps per-namespace KEKs stored in cluster-level controller storage. Nodes unwrap data keys only for containers they host. Rotate master key = rewrap KEKs. Backup the master key. Restore with a specified key. Lost key + no backup ⇒ encrypted data gone.
- Q: Is encryption at rest mandatory for SaaS? → A: Opt-in per container (`03`). Stolen disk of **encrypted** containers is ciphertext. Unencrypted persistent data is an operator-chosen leak. A running node that unwrapped keys can read what it hosts. `CLUSTER_ADMIN` can unwrap. Not enclave/confidential computing.

### Session 2026-09-16

- Q: Must principal login names be changeable? → A: Yes. Principal **id** is immutable. **Login name** is a unique, renameable label. `07` bindings and audit target the id. After rename, new authentications MUST use the new login; in-flight sessions stay bound by **id** for namespace identity until disconnect. Password change, disable, and grant reduction are not covered by that identity rule (see 2026-09-18).

### Session 2026-09-18

- Q: How should a cluster administrator open a Redis, S3, WebDAV, or Elasticsearch session, given those protocols take the namespace from the login and refuse a login that is not bound to exactly one namespace? → A: Unbound `admin` / `CLUSTER_ADMIN` is refused on Redis, S3, WebDAV, and Elasticsearch (same as any other unbound credential). Admin work there uses a namespace-bound principal, or a protocol/API that can select a namespace (PostgreSQL / Cassandra / ClickHouse native select, or the admin CLI/HTTP).
- Q: How is the first cluster-admin login created when a new cluster is bootstrapped? → A: The bootstrap declaration includes the initial admin login and password (or verifier). That principal is `CLUSTER_ADMIN`. An empty principal store MUST NOT leave admin CLI/HTTP open. The join secret MUST NOT authenticate as an administrator.
- Q: Which parts of logins, permissions, keys, and audit must already work in the first shippable binary, and which wait for later slices? → A: First binary: local principal store, bootstrap `CLUSTER_ADMIN`, SCRAM and Redis AUTH, one-namespace binding, built-in `admin` and `replication` only, `tls`/`plaintext;`, master-key file wrapping namespace KEKs, audit of join / key bind-rotate / failed auth. Custom roles and the rest of the permission list wait for slice 7 (`16`). Rewriting data under a new data key waits for transforms (`10`).
- Q: What permissions do the built-in `admin` and `replication` roles carry, and does `CLUSTER_ADMIN` already include the other verbs (read, write, audit, and the rest)? → A: Built-in `admin` is `{CLUSTER_ADMIN}`, and that verb includes every other verb in the list. Built-in `replication` is `{REPLICATE}` only and is not a tenant login. Neither built-in role can be edited.
- Q: After a login's password is changed, the principal is disabled, or its permissions are reduced, what happens to connections that are already open? → A: Later requests re-check enablement, password (credential generation), and current grants. In-flight work MAY finish. No forced disconnect. New authentications use the new password and grants.

### Session 2026-09-19

- Q: What is the normative SCRAM-SHA-256 PBKDF2 iteration count? → A: Default **16384** (`auth.scram_iterations`). Tests MAY override to **4096**. Production MAY raise above 16384 (documented range up to 65536); 16384 is the shipped default, not 4096.
- Q: Until custom roles (slice 7), what may a namespace-bound tenant principal do on its namespace in the first binary? → A: A principal bound to exactly one namespace that is not `admin` or `replication` receives an **implicit** grant `{READ, WRITE, CREATE, DROP, CONFIGURE}` on that namespace only. This is not a stored custom role; slice 7 replaces it with explicit custom roles. Needed so Redis/PG smoke can use a bound login while unbound `admin` is refused on Redis.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Authenticate and bind a namespace (Priority: P1)

A tenant uses a stock client. PostgreSQL uses SCRAM; Redis AUTH maps to the same principal store. Non-admin principals are bound to exactly one namespace. A credential with no namespace binding — **including unbound `admin` / `CLUSTER_ADMIN`** — is refused on tenant protocols that have no native select-database step (Redis, S3, WebDAV, Elasticsearch). Cluster administrators cross namespaces on those protocols by using a principal bound to the target namespace, or by using PostgreSQL / Cassandra / ClickHouse native select (where `CLUSTER_ADMIN` MAY select any namespace) or the admin CLI/HTTP. Admin CLI/HTTP uses bearer token or the same store. The first `CLUSTER_ADMIN` is created from the bootstrap declaration (login + password or verifier), not from an open admin port and not from the join secret. Until verified, a session MUST NOT see tenant data.

The **first shippable binary** (`16` slices 1–5) MUST implement this story for PostgreSQL and Redis (the protocols that binary ships). The same unbound-refuse rule applies to S3, WebDAV, and Elasticsearch when those handlers exist (slice 6). `NAMESPACE_ADMIN` as a grantable permission waits for **slice 7**; first-binary login rename is by `CLUSTER_ADMIN` or by the principal themselves.

**Why this priority**: `02` already assumed credential-bound namespaces.

**Independent Test**: Bootstrap with initial admin; authenticate admin CLI; try join secret as admin (refuse); create principal bound to `acme`; connect PG and Redis; try unbound Redis including the `admin` login; try `otherns`; rename the login and authenticate with the new name; change password on an open session and issue another request.

**Acceptance Scenarios**:

1. **Given** a principal bound to `acme` with valid password, **When** it authenticates on PostgreSQL (SCRAM-SHA-256) and Redis AUTH, **Then** both sessions see only `acme`.
2. **Given** a credential with no namespace binding (tenant or unbound `admin` / `CLUSTER_ADMIN`), **When** it authenticates on Redis/S3/WebDAV/Elasticsearch, **Then** authentication is refused. **Given** `CLUSTER_ADMIN` on PostgreSQL (database name), Cassandra (keyspace), ClickHouse (database), or admin CLI/HTTP, **When** it selects an existing namespace, **Then** the session is bound to that namespace.
3. **Given** PostgreSQL database name / Cassandra keyspace / ClickHouse database, **When** the principal is not allowed that namespace, **Then** the session is refused even though the protocol has a native select step.
4. **Given** an unverified session, **When** it requests tenant data, **Then** nothing is returned.
5. **Given** principal id `P` with login `alice` (first binary), **When** `CLUSTER_ADMIN` or `alice` herself renames the login to `ally`, **Then** id `P` and namespace binding are unchanged, new AUTH/`psql -U ally` succeed, `alice` no longer authenticates that principal, and a taken target login is refused. **Given** slice 7, **When** a `NAMESPACE_ADMIN` for `acme` renames a principal bound to `acme`, **Then** the same id/binding rules apply; **When** they rename a principal not bound to `acme`, **Then** it is refused.
6. **Given** first-node bootstrap (`11`) with an initial admin login and password (or verifier), **When** the node reaches `ready`, **Then** that principal exists as `CLUSTER_ADMIN` and authenticates on admin CLI/HTTP. **Given** the join secret and no admin login, **When** it is presented to admin CLI/HTTP or a tenant protocol, **Then** it is refused. **Given** bootstrap with no initial admin credential, **When** the node starts, **Then** startup fails. **Given** restart of an already-bootstrapped node, **When** bootstrap is still declared, **Then** the existing principal store is kept (no second bootstrap admin).
7. **Given** an open session (first binary), **When** that principal's password is changed (self or `CLUSTER_ADMIN`) or the principal is disabled, **Then** a request already in flight MAY finish, the TCP connection NEED NOT be dropped, and the next request is refused. **Given** login rename only, **When** a later request arrives on the same session, **Then** it still runs as that principal id (namespace identity unchanged). **Given** slice 7 grant reduction, **When** the next request needs a stripped verb, **Then** it is refused; a request already in flight MAY finish.

---

### User Story 2 - Permission vocabulary and roles (Priority: P1)

Roles `admin`, `replication`, and `custom` (`07`) are composed from a closed vocabulary: `CLUSTER_ADMIN`, `NAMESPACE_ADMIN`, `READ`, `WRITE`, `CREATE`, `DROP`, `CONFIGURE`, `REPLICATE`, `MIGRATE`, `AUDIT_READ`, `METRICS_READ`. Built-in `admin` is the set `{CLUSTER_ADMIN}`; **`CLUSTER_ADMIN` implies every other verb** (all namespaces, data plane, keys, audit, metrics, migrate, membership). Built-in `replication` is `{REPLICATE}` only and MUST NOT be a tenant login. Neither built-in role is editable. A custom role that includes `CLUSTER_ADMIN` (slice 7) has the same implication. UIs (`09`) MUST use the same vocabulary. Replication authenticates nodes on `internode`/`replication`, not tenants.

The **first binary** MUST persist and honor built-in `admin` (`CLUSTER_ADMIN`) and `replication` (`REPLICATE`) only. Until slice 7, a namespace-bound non-admin principal receives an **implicit** `{READ, WRITE, CREATE, DROP, CONFIGURE}` on that namespace (FR-017). **Custom** roles and attaching remaining verbs as grants (without `CLUSTER_ADMIN`) wait for **slice 7** (with `07`). UIs wait for `09`.

**Why this priority**: Three role names were unimplementable without verbs.

**Independent Test**: First binary: CLUSTER_ADMIN join and CREATE/WRITE without extra grants; REPLICATE on internode not on tenant data; edit of built-in `admin` refused; custom-role create refused. Slice 7: custom READ-only; custom with `CLUSTER_ADMIN` implies WRITE. After `09`: UI attempt with extra privilege (must fail).

**Acceptance Scenarios**:

1. **Given** a custom role with `READ` only (slice 7), **When** the principal writes or `CREATE`s, **Then** those attempts are refused.
2. **Given** `CLUSTER_ADMIN` (first binary) with no separate `READ`/`WRITE`/`CREATE` grants, **When** it admits a join (`11`), changes global config, or creates and writes a container in any namespace, **Then** those actions succeed and are audit-logged.
3. **Given** `REPLICATE` (first binary), **When** used on tenant protocols, **Then** it does not grant tenant data; **When** used on `internode`/`replication`, **Then** peers authenticate.
4. **Given** first binary, **When** a caller creates a custom role, grants `READ`/`NAMESPACE_ADMIN`/`AUDIT_READ` as a custom set, or edits built-in `admin` or `replication` (strip or add verbs), **Then** the attempt is refused or documented as not in this binary.
5. **Given** a principal bound to exactly one namespace (first binary, not `admin`/`replication`), **When** it authenticates on Redis/PostgreSQL and issues data-plane ops needing `READ`/`WRITE`/`CREATE`/`DROP`/`CONFIGURE` on that namespace, **Then** those succeed via the implicit grant (FR-017) without a stored custom role.
6. **Given** a custom role that includes `CLUSTER_ADMIN` (slice 7), **When** the principal writes or reads audit without those verbs listed separately, **Then** those actions succeed.
7. **Given** a UI (`09`), **When** it attempts an action, **Then** the same vocabulary is enforced; no private privilege model.

---

### User Story 3 - Transport and keys (Priority: P1)

Every entrypoint declares `tls` or `plaintext;`. Envelope encryption: master key wraps namespace KEKs; data keys encrypt payloads and WAL/snapshots (`13`). Rotate master = rewrap. A new data key MAY be issued; old keys are retained until a transform (`10`) re-encrypts. First binary: master-key file, no required external KMS. Rewriting existing data under a new data key waits for **`10`**.

**Why this priority**: Grill Q6/Q7/Q12/Q13/Q15.

**Independent Test**: First binary: omit transport (fail start); encrypt a container; steal a volume snapshot (ciphertext); rotate master; restore with specified key. After `10`: transform re-encrypts under the new data key.

**Acceptance Scenarios**:

1. **Given** an entrypoint with neither `tls` nor `plaintext;` (first binary), **When** the node starts, **Then** startup fails.
2. **Given** `tls` declared (first binary), **When** a plaintext client connects, **Then** it is refused; no silent fallback.
3. **Given** an encrypted container (first binary), **When** a disk/snapshot is taken without the master key, **Then** the data is ciphertext.
4. **Given** master-key rotate (first binary), **When** it completes, **Then** KEKs are rewrapped, data keys unchanged, no table rewrite.
5. **Given** lost master key without backup, **When** restore is attempted, **Then** encrypted containers (and those backups) are unreadable and the error says so; unencrypted containers are unaffected.
6. **Given** restore with a specified key (`13`, first binary), **When** the key is correct, **Then** encrypted snapshots unlock.
7. **Given** a new data key issued for a container (complete product), **When** existing rows are still under the old key, **Then** they remain readable until a transform (`10`) re-encrypts; first binary MUST NOT require that rewrite.

---

### User Story 4 - Audit (Priority: P2)

Privileged actions append to an audit log: auth success/failure, role changes, membership join/leave/replace, key bind/rotate, backup/restore, migrate/transform, admin config, **principal login rename**, **namespace rename** (namespace rename is executed in `07`; audit is this feature). Entries include principal **id**, action, target, time (HLC/`12`). Tenants do not see cluster audit unless granted `AUDIT_READ`.

The **first binary** MUST append join, key bind/rotate, failed authentication, login rename, and namespace rename. `CLUSTER_ADMIN` MAY read that log. Granting `AUDIT_READ` to a tenant custom role, and refusing tenants without it, wait for **slice 7**. Other event types appear when those features exist (`09`/`10`).

**Why this priority**: Admit tokens and key rotate are otherwise unauditable.

**Independent Test**: First binary: join, key rotate, failed login; `CLUSTER_ADMIN` reads audit. Slice 7: tenant without `AUDIT_READ` is refused; tenant with it can read.

**Acceptance Scenarios**:

1. **Given** a join, key rotate, and failed authentication (first binary), **When** audit is read by `CLUSTER_ADMIN`, **Then** all three appear with principal, action, target, and time.
2. **Given** a tenant without `AUDIT_READ` (slice 7), **When** they request cluster audit, **Then** they are refused.

---

### Edge Cases

- mTLS optional per entrypoint; certificate referenced, never inlined.
- LDAP/Kerberos later; first binary is the local principal store in controller storage.
- Threshold/split master keys later. External KMS later. Custom roles and remaining verbs: slice 7. Data-key rewrite: `10`. UIs: `09`. Remaining protocol handlers: slice 6 (`16`).
- Who may bind a key reference: `CLUSTER_ADMIN` in the first binary; `NAMESPACE_ADMIN` for that namespace from slice 7.
- Lost data key → that container unreadable; error names the missing key reference.
- Login rename: password/SCRAM verifier unchanged. Duplicate login → refused. `replication` node identities are not tenant logins. Audit records principal **id**; MAY also record login at event time. Open sessions stay bound by id (namespace identity) until disconnect; rename alone MUST NOT fail later requests on that session.
- Password change, disable, or grant reduction: no forced disconnect. Each later request MUST re-check that the principal is enabled, the credential generation still matches the session, and current grants allow the action. In-flight work MAY finish. New authentications use the new password and grants.
- Unbound `admin` / `CLUSTER_ADMIN` on Redis, S3, WebDAV, or Elasticsearch: refused at authentication, same as any unbound credential. No connect option, header, or Redis `SELECT` bypass. Admin data-plane on those protocols uses a namespace-bound principal.
- First `CLUSTER_ADMIN`: created only at cluster bootstrap from the declared login + password/verifier. Empty principal store MUST NOT accept unauthenticated admin calls (loopback included). Join secret is membership-only (`11`) and MUST NOT grant `CLUSTER_ADMIN`. Missing bootstrap admin credential → startup error. Re-declaring bootstrap on a node that already has a cluster identity MUST NOT recreate or reset that principal.
- Built-in `admin` / `replication` mutation (rename of the role, strip/add verbs, delete): refused. A custom role MUST NOT be named `admin` or `replication`.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: A principal is a cluster-wide identity with an immutable id and a unique login name. Auth uses each client protocol's native mechanism (`02`) and admin token/store (`01`). SCRAM-SHA-256 MUST be implemented. Optional mTLS per entrypoint.
- **FR-002**: Until verified, a session MUST NOT see tenant data. Non-admin principals MUST be bound to exactly one namespace. Unbound credentials — including unbound `admin` / `CLUSTER_ADMIN` — MUST be refused on Redis, S3, WebDAV, Elasticsearch (and any protocol without native select-database). Those protocols MUST NOT offer an admin-only namespace selector (no connect option, header, or Redis `SELECT` as namespace switch). Native select still MUST authenticate and MUST only select an allowed namespace; `CLUSTER_ADMIN` MAY select any existing namespace on PostgreSQL, Cassandra, ClickHouse, and admin CLI/HTTP. `admin` MAY cross namespaces only by opening a new session that is allowed for that namespace (bound principal, or native select / admin API). `replication` authenticates nodes, not tenants.
- **FR-003**: Custom roles MUST be composed from the closed vocabulary: `CLUSTER_ADMIN`, `NAMESPACE_ADMIN`, `READ`, `WRITE`, `CREATE`, `DROP`, `CONFIGURE`, `REPLICATE`, `MIGRATE`, `AUDIT_READ`, `METRICS_READ`. Built-in `admin` MUST be exactly `{CLUSTER_ADMIN}`. Built-in `replication` MUST be exactly `{REPLICATE}` and MUST NOT authenticate as a tenant. **`CLUSTER_ADMIN` implies every other verb** in that list (any principal that has it, including a slice-7 custom role that includes it). Built-in `admin` and `replication` MUST NOT be edited, renamed, or deleted. Custom roles and attaching those verbs except via the two built-ins are **slice 7**.
- **FR-004**: UIs MUST enforce the same vocabulary (`09`). This MUST NOT be required before `09`.
- **FR-005**: Every entrypoint MUST declare `tls { ... }` or `plaintext;`. Omitted transport is a startup error. TLS is not globally mandatory. Certificate material referenced, never inlined. No silent plaintext fallback when TLS is declared.
- **FR-006**: Encryption at rest is opt-in per container (`03`). Encrypted: stolen disk/snapshot/backup is ciphertext. Running node that unwrapped keys can read hosted data. Nodes **cache** unwrapped data keys only for containers they **host**; `CLUSTER_ADMIN` MAY unwrap for restore/admin outside that hosted-only cache. Unencrypted persistent data is an operator-chosen leak.
- **FR-007**: Algorithms MUST include AES-256-GCM (default) and ChaCha20-Poly1305.
- **FR-008**: Envelope: cluster master key (file in first binary; later KMS as another provider, same references) wraps per-namespace KEKs in cluster-level controller storage. Nodes unwrap data keys only for containers they host and cache them in memory. Data keys encrypt payloads and WAL/snapshots (`13`).
- **FR-009**: Rotate master key = rewrap KEKs; data keys unchanged. Master key MUST be backupable. Restore with a specified key MUST be documented. Lost master without backup ⇒ encrypted containers unrestorable, explicit.
- **FR-010**: Data-key rotate MAY issue a new key; old keys retained until transform (`10`). Lost data key → container unreadable, error names the reference. Rewriting existing payloads under the new key MUST NOT be required before `10`.
- **FR-011**: Bind key reference: `CLUSTER_ADMIN` for any namespace (first binary). `NAMESPACE_ADMIN` MAY bind a key reference for their namespace from **slice 7**.
- **FR-012**: Audit log for privileged actions as listed, with principal id, action, target, time. Login rename and namespace rename MUST appear. First binary MUST append join, key bind/rotate, failed authentication, **login rename**, and **namespace rename**, readable by `CLUSTER_ADMIN`. Tenants need `AUDIT_READ` to see cluster audit; that grant is **slice 7**.
- **FR-013**: The first shippable binary (`16` slices 1–5) MUST include: local principal store in cluster-level controller storage; bootstrap `CLUSTER_ADMIN` (FR-015); SCRAM-SHA-256 and Redis AUTH mapped to that store; one-namespace binding for non-admin principals; built-in `admin` and `replication` only; implicit bound-tenant grant (FR-017); every entrypoint `tls` or `plaintext;`; cluster master-key file wrapping namespace KEKs; audit of join, key bind/rotate, failed auth, login rename, and namespace rename. It MUST NOT require custom roles, tenant `AUDIT_READ`, remaining protocol handlers, UIs, external KMS, or data-key rewrite via transform. LDAP/SSO later.
- **FR-014**: A principal MUST have an immutable id and a unique, **renameable** login name (same charset class as namespace names in `07`: 1–63, `^[A-Za-z][A-Za-z0-9_]*$`). Rename MUST NOT change id, password verifier, or namespace binding. After rename, the old login MUST NOT authenticate that principal. Who MAY rename: the principal themselves; `CLUSTER_ADMIN` for any principal (first binary); `NAMESPACE_ADMIN` for principals bound to their namespace (**slice 7**). Taken name → refused. Role bindings in `07` MUST use principal id.
- **FR-015**: Cluster bootstrap (`11`) MUST create the first `CLUSTER_ADMIN` principal from an initial admin login and password (or verifier) in the bootstrap declaration. Bootstrap without that credential MUST fail startup. An empty principal store MUST NOT leave admin CLI/HTTP unauthenticated (including loopback). The join secret MUST NOT authenticate as `CLUSTER_ADMIN` or any tenant principal. Restart of a node that already has a cluster identity MUST keep the existing principal store and MUST NOT mint a second bootstrap admin.
- **FR-016**: After a session is authenticated, each later request MUST re-check principal enablement, credential generation (password/verifier change invalidates the session's right to continue), and current grants. A request already in flight MAY finish. The server MUST NOT be required to drop the connection. Login rename MUST NOT by itself fail later requests (session remains bound by principal id). New authentications MUST use the current login, password, and grants. Password change (self or `CLUSTER_ADMIN`) is first binary; disable follows the same re-check; grant reduction applies from slice 7.
- **FR-017**: Until custom roles (slice 7), a principal bound to exactly one namespace that is not `admin` or `replication` MUST receive an **implicit** grant `{READ, WRITE, CREATE, DROP, CONFIGURE}` on that namespace only. The grant is not a stored custom role and MUST NOT invent custom RolePut in the first binary. Slice 7 replaces it with explicit custom roles (empty custom set = authenticate only).

### Key Entities

- **Principal**: Cluster-wide identity. Attributes: immutable id, unique renameable login name, credential material (`14`), optional namespace binding (`07`).
- **Credential Namespace Binding**
- **Permission**: One verb from the closed vocabulary.
- **Role**: `admin` = `{CLUSTER_ADMIN}` (implies all other verbs); `replication` = `{REPLICATE}`; `custom` = an explicit set of verbs (`07` stores; this feature defines verbs and implication). Built-in roles are immutable.
- **Master Key / Namespace KEK / Data Key**: Envelope hierarchy.
- **Audit Entry**: Privileged action record.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: First binary: 100% of unbound Redis authentications in the suite are refused, including unbound `admin` / `CLUSTER_ADMIN`; 100% of bound principals see only their namespace. Slice 6+: the same 100% refuse/bind figures hold for S3, WebDAV, and Elasticsearch. 100% of `CLUSTER_ADMIN` native-select sessions in the suite bind to the selected namespace.
- **SC-002**: Slice 7: 100% of READ-only custom roles cannot write; 100% of custom roles that include `CLUSTER_ADMIN` succeed at write/audit without those verbs listed separately. After `09`: 100% of UI actions use the same vocabulary. First binary: 100% of custom-role create attempts and 100% of built-in role edits in the suite are refused or absent; 100% of `CLUSTER_ADMIN` smoke writes/creates succeed with no extra grants.
- **SC-003**: 100% of entrypoints that omit transport fail startup; 100% of TLS entrypoints refuse plaintext.
- **SC-004**: 100% of encrypted-container disk images in the suite are unreadable without the master key; 100% of unencrypted containers remain readable (chosen leak).
- **SC-005**: Master-key rotate rewraps KEKs with 0 table rewrites in 100% of tests.
- **SC-006**: First binary: 100% of join, key-rotate, failed-auth, login-rename, and namespace-rename events in the suite appear in audit with principal, action, target, time, readable by `CLUSTER_ADMIN`. Slice 7: 100% of tenant requests without `AUDIT_READ` are refused.
- **SC-007**: 100% of login renames in the suite keep principal id and namespace binding; 100% of new auths with the old login fail for that principal; 100% of duplicate target logins are refused.
- **SC-008**: 100% of first-node bootstraps in the suite with an initial admin credential yield a working `CLUSTER_ADMIN`; 100% of bootstraps without that credential fail startup; 100% of join-secret-as-admin attempts are refused; 100% of restarts keep the existing bootstrap admin (no duplicate).
- **SC-009**: 100% of later requests in the suite after password change or disable are refused; 100% of login-rename-only later requests still run as that principal id; 0 forced disconnects required. Slice 7: 100% of later requests that need a stripped verb are refused.

## Assumptions

- Role **names** and quota units are `07`. Encryption **scope** field is `03`. Internode framing is `12`. WAL encryption uses this feature's data keys (`13`).
- Hostile tenants; operator runs nodes; crash-stop (`16`).
- First binary has no external KMS. Slice numbering follows `16`; first binary is slices 1–5. Custom roles wait for slice 7 (`07`/`14`); until then FR-017 implicit tenant grant applies. Data-key rewrite waits for `10`.
- Default SCRAM-SHA-256 PBKDF2 iterations = **16384** (`auth.scram_iterations`); tests MAY use **4096**.
- Join secret (`11`) is necessary to speak `internode`/`replication` and is never an admin or tenant credential.

## Out of Scope

- Namespace and quota **units** (`07`).
- Type inventory and encryption scope (`03`).
- Internode protocol framing (`12`).
- Cerebro/Kibana chrome (`09`) except same authz.
- LDAP/SSO (future).
- Custom roles, tenant `AUDIT_READ`, and remaining permission grants as **first-binary** requirements (they remain complete-product / slice 7).
- Data-key rewrite via transform as a **first-binary** requirement (`10`).
