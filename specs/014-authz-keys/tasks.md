---
description: "Task list for authentication, authorization, envelope keys, and audit"
---

# Tasks: Authentication, Authorization, Encryption in Transit, Keys, and Audit

**Input**: Design documents from `/specs/014-authz-keys/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md), [data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: Requested. Plan **Testing** + Independent Tests + SC-001–SC-009 + conformance in `crates/conformance`. Unit tests for SCRAM verify, implication, generation bump, unbound Redis refuse, rewrap, audit fields. Contract fixtures under `contracts/fixtures/`. Write failing tests first where listed.

**Scope of this feature**: New `crates/authz` (`spacestorage-authz`); envelope `KeyAuthority` in existing `crates/crypto`; cluster-log bodies via `controlplane`; replace `002` `auth.users_file` and `003` `keyring_file`; bootstrap first `CLUSTER_ADMIN`; first-binary profile only (custom roles / tenant `AUDIT_READ` / `NAMESPACE_ADMIN` ops behind `authz-custom` slice 7). No new binary. Do not reimplement TLS grammar (`001`) or role/quota storage (`007`).

**Sibling crates** (wire seams; do not duplicate their internals): `crates/crypto`, `crates/tenancy`, `crates/controlplane`, `crates/config`, `crates/node`, `crates/handler-postgresql`, `crates/handler-redis`, `crates/internodes`, `crates/membership`, `crates/admin-proto`, `crates/spacestorage`, `crates/release-profile`, `crates/conformance`.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: [US1], [US2], [US3], [US4] on user-story phase tasks only
- Every task includes an exact file path

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Workspace member, crate skeleton, feature flags, fixture docs tree

- [ ] T001 Create `crates/authz/Cargo.toml` (package `spacestorage-authz`, edition 2024) with workspace deps `tokio`, `async-trait`, `serde`, `serde_json`, `bytes`, `tracing`, `uuid`, `parking_lot`, `subtle`, `zeroize`, `hmac`, `sha2`, `pbkdf2` and `crates/authz/src/lib.rs` that `mod`s `principal`, `scram`, `permission`, `session`, `bootstrap`, `audit`
- [ ] T002 Add `crates/authz` to workspace `[workspace.members]` in `Cargo.toml` and declare Cargo feature `authz-custom` (slice 7; off by default) in `crates/authz/Cargo.toml` and wire `first-binary` compile of `authz` (without custom RolePut) in `crates/release-profile`
- [ ] T003 [P] Ensure `docs/examples/` (or feature docs tree) references [contracts/fixtures/bootstrap-admin.conf](contracts/fixtures/bootstrap-admin.conf) and [contracts/fixtures/master-key.conf](contracts/fixtures/master-key.conf); keep invalid fixtures under `specs/014-authz-keys/contracts/fixtures/invalid/`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Shared types, validation codes, cluster-log body shapes, and crypto/master-file primitives every user story needs. No user-story work until this phase is complete.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [ ] T004 Implement closed `Verb` bitmask in `crates/authz/src/permission.rs` with exactly `CLUSTER_ADMIN`, `NAMESPACE_ADMIN`, `READ`, `WRITE`, `CREATE`, `DROP`, `CONFIGURE`, `REPLICATE`, `MIGRATE`, `AUDIT_READ`, `METRICS_READ`; `CLUSTER_ADMIN` **implies every other verb** and every namespace
- [ ] T005 [P] Implement `PrincipalId` (UUID, immutable) and `LoginName` in `crates/authz/src/principal.rs` with login constraint verbatim: unique cluster-wide, case-sensitive, `^[A-Za-z][A-Za-z0-9_]*$`, 1–63 chars; **renameable**
- [ ] T006 [P] Implement `PrincipalRecord` in `crates/authz/src/principal.rs` with fields `id`, `login`, `scram` (salt, iteration count, StoredKey, ServerKey — never plaintext), `credential_generation` (u64, starts at 1; +1 on password change), `enabled` (bool, default true), `created_hlc`, `bootstrap` (bool, true only for the first CLUSTER_ADMIN mint); unique login index; duplicate → `LoginExists`
- [ ] T007 [P] Implement in-memory `Session` in `crates/authz/src/session.rs` with `principal_id`, optional `namespace_id`, `credential_generation` copied at AUTH, `protocol`; export `recheck` that loads PrincipalRecord and refuses on `!enabled` (`PrincipalDisabled`) or generation mismatch (`AuthGenerationMismatch`) then `authorize` on current bindings
- [ ] T008 [P] Implement optional `SessionToken` row type in `crates/authz/src/session.rs` (cluster log): `token_hash` SHA-256 of presented secret, `principal_id`, `generation` at issue, `expires_hlc` (TTL default 12 h)
- [ ] T009 Export normative validation codes in `crates/authz/src/lib.rs` (or `error.rs`): `BootstrapAdminRequired`, `JoinSecretNotAdmin`, `UnboundCredential`, `ReplicationNotTenant`, `LoginExists`, `LoginNotFound`, `BuiltinRoleImmutable`, `RoleNameReserved`, `Slice7Required`, `AuthGenerationMismatch`, `PrincipalDisabled`, `MasterKeyRequired`, `MasterKeyPermissions`, `KeyUnresolvable`, `KeyMaterialForbidden`, `TransportOmitted`, `AdminTokenRemoved`, `UsersFileRemoved`, `KeyringRemoved`, `DataKeyRewriteNotFirstBinary`
- [ ] T010 Register cluster-log body types for `Principal*` / `Kek*` / `Audit*` / `SessionToken*` in `crates/controlplane` (or authz→controlplane bridge) so bodies apply from the **cluster** Raft group `{data_dir}/raft/cluster/`
- [ ] T011 [P] Implement `MasterKey` file IO in `crates/crypto/src/master_file.rs`: 32 bytes at `keys.master_key_file`, mode `0600`, never logged; missing/wrong mode → `MasterKeyRequired` / `MasterKeyPermissions`
- [ ] T012 [P] Implement `KekRecord` and wrap/unwrap helpers in `crates/crypto/src/envelope.rs`: `namespace_id`, `wrapped` AEAD blob under current master, `kek_epoch` (u64; +1 on master rewrap only); AES-256-GCM wrap of 32-byte KEK
- [ ] T013 Define `DataKey` shape on container definition seam in `crates/crypto/src/envelope.rs` (or `003` container types): `key_ref` (opaque string id), `algorithm` `aes-256-gcm` (default) \| `chacha20-poly1305`, `wrapped` under namespace KEK, `version` u32 (old versions retained until `010`); lost/missing → `KeyUnresolvable{key_ref}`
- [ ] T014 [P] Implement `AuditEntry` struct in `crates/authz/src/audit.rs` with `id` UUID, `principal_id` (or nil for failed AUTH with unknown login), optional `login_at_event`, `action`, `target`, optional `namespace_id`, `time` Hlc; key material MUST NEVER appear

**Checkpoint**: `cargo check -p spacestorage-authz` and `cargo check -p spacestorage-crypto` compile with types and codes. User stories can start.

---

## Phase 3: User Story 1 - Authenticate and bind a namespace (Priority: P1) 🎯 MVP

**Goal**: Local principal store; bootstrap first `CLUSTER_ADMIN`; SCRAM-SHA-256 + Redis AUTH to the same store; one-namespace binding for non-admin; unbound credentials (including unbound `admin` / `CLUSTER_ADMIN`) refused on Redis; login rename; password change bumps generation and later requests refuse; join secret never admin; replace `auth.users_file` / `admin.token_file` as auth sources.

**Independent Test**: Bootstrap with initial admin; authenticate admin CLI; try join secret as admin (refuse); create principal bound to `acme`; connect PG and Redis; try unbound Redis including `admin`; try `otherns`; rename login and auth with new name; change password on open session and issue another request. SC-001, SC-007, SC-008, SC-009.

### Tests for User Story 1 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T015 [P] [US1] Add unit tests in `crates/authz/src/scram.rs` (or `crates/authz/tests/scram.rs`) for SCRAM-SHA-256 verify, iteration from `auth.scram_iterations` (default 16384; test override 4096), and Redis AUTH off-wire verify against the same verifier
- [ ] T016 [P] [US1] Add config validate tests in `crates/config` covering [contracts/fixtures/invalid/](contracts/fixtures/invalid/): `BootstrapAdminRequired`, `UsersFileRemoved`, `AdminTokenRemoved`, `KeyMaterialForbidden` per [quickstart.md](quickstart.md) §0
- [ ] T017 [P] [US1] Add conformance tests in `crates/conformance` for SC-008 bootstrap admin: [bootstrap-admin.conf](contracts/fixtures/bootstrap-admin.conf) → ready + `spacestorage login` as admin; join secret as password → `JoinSecretNotAdmin`; restart with `bootstrap;` still declared → no second bootstrap admin
- [ ] T018 [P] [US1] Add conformance tests in `crates/conformance` for SC-001: bound `alice` on PG+Redis sees only `acme`; `redis-cli AUTH admin` → `UnboundCredential`; `psql -U admin -d acme` succeeds; `psql -U alice -d otherns` refused
- [ ] T019 [P] [US1] Add conformance tests in `crates/conformance` for SC-007 / SC-009: rename `alice`→`ally`; old login fails; open session keeps principal id; password change → next request `AuthGenerationMismatch` without forced TCP drop

### Implementation for User Story 1

- [ ] T020 [P] [US1] Implement SCRAM-SHA-256 salt/StoredKey/ServerKey derive + verify in `crates/authz/src/scram.rs`; run high-iteration PBKDF2 on `spawn_blocking`; never store plaintext
- [ ] T021 [US1] Implement `Authenticator::authenticate(protocol, identity, secret)` in `crates/authz/src/lib.rs` per [contracts/authenticator.md](contracts/authenticator.md); until success, session MUST NOT see tenant data
- [ ] T022 [US1] Implement bootstrap mint in `crates/authz/src/bootstrap.rs`: on `cluster { bootstrap; admin_login L; admin_password_file P; }` with no persisted `identity/cluster.json`, create one `PrincipalRecord` (`bootstrap=true`) + binding to builtin `admin`; missing login/file → `BootstrapAdminRequired`; existing cluster identity → skip mint (no second admin)
- [ ] T023 [US1] Wire bootstrap + principal store into `crates/node` startup and refuse start when principal store empty without bootstrap admin; empty store MUST NOT leave admin CLI/HTTP open (including loopback)
- [ ] T024 [US1] Add config directives in `crates/config`: `cluster.admin_login`, `cluster.admin_password_file`, `auth.scram_iterations` (default 16384); reject `auth.users_file` → `UsersFileRemoved`; reject `admin.token_file` as CLUSTER_ADMIN source → `AdminTokenRemoved` per [contracts/config-directives.md](contracts/config-directives.md)
- [ ] T025 [US1] Replace `002` users_file authenticator: wire `crates/handler-postgresql` SCRAM exchange to `authz::Authenticator`; map client database name; non-allowed namespace → refuse even with native select; `CLUSTER_ADMIN` MAY select any existing namespace
- [ ] T026 [US1] Wire `crates/handler-redis` AUTH to `authz::Authenticator`; no namespace binding → `UnboundCredential` (including unbound `admin` / `CLUSTER_ADMIN`); no header/connect-option/Redis `SELECT` bypass
- [ ] T027 [US1] Presenting `cluster.token_file` (join secret) to tenant or admin AUTH → `JoinSecretNotAdmin` in `crates/authz` + `crates/membership` / admin path; join secret MUST NOT authenticate as administrator
- [ ] T028 [US1] Implement principal CRUD ops used by admin: `PrincipalCreate` (non-admin MUST bind exactly one `namespace_id`), `PrincipalRename`, `PrincipalPasswordSet` (+1 `credential_generation`), `PrincipalDisable`/`Enable`/`Delete` (tombstone; id not reused) in `crates/authz/src/principal.rs` + cluster-log apply
- [ ] T029 [US1] First-binary rename/password who-MAY rules in `crates/authz`: self or `CLUSTER_ADMIN` for any principal; taken login → `LoginExists`; rename MUST NOT change id, verifier, generation, or `007` bindings
- [ ] T030 [US1] Implement later-request re-check on handler request path in `crates/authz/src/session.rs` + PG/Redis handlers: enablement + generation + current grants; in-flight MAY finish; TCP NEED NOT drop; login rename alone MUST NOT fail later requests
- [ ] T031 [US1] Implement optional admin bearer `AuthLogin` → 32-byte token, store SHA-256 `SessionToken` in cluster log (TTL 12 h) in `crates/admin-proto` + `crates/authz/src/session.rs`; first binary MAY use password on every admin call instead
- [ ] T032 [US1] Add admin CLI/API ops in `crates/admin-proto` and `crates/spacestorage`: `spacestorage login`, `principals`, `principal create <login> --namespace <ns> --password-file P`, `principal rename`, `principal passwd` per [contracts/admin-cli.md](contracts/admin-cli.md); exit 4 for authz errors
- [ ] T033 [US1] Document operator protocol-mapping table from [contracts/protocol-mapping.md](contracts/protocol-mapping.md) in starter docs (`002` FR-043 seam) so Redis/S3/WebDAV/ES unbound-admin refuse is explicit
- [ ] T034 [US1] Gate slice-6 handler unbound-refuse behind handler presence: same `UnboundCredential` rule for S3/WebDAV/ES when those handlers exist (no first-binary requirement to ship handlers)

**Checkpoint**: Bootstrap admin works; PG+Redis bound smoke; unbound Redis admin refused; rename + password re-check pass SC-001/007/008/009 independently of keys/audit polish.

---

## Phase 4: User Story 2 - Permission vocabulary and roles (Priority: P1)

**Goal**: Closed verb vocabulary; builtin `admin`=`{CLUSTER_ADMIN}` (implies all verbs) and `replication`=`{REPLICATE}` immutable; first-binary implicit tenant grant `{READ,WRITE,CREATE,DROP,CONFIGURE}` on bound namespace; custom RolePut → `Slice7Required`; replication not a tenant login.

**Independent Test**: First binary: CLUSTER_ADMIN join and CREATE/WRITE without extra grants; REPLICATE on internode not on tenant data; edit of builtin `admin` refused; custom-role create refused. SC-002.

### Tests for User Story 2 ⚠️

- [ ] T035 [P] [US2] Add unit tests in `crates/authz/src/permission.rs` for implication: principal with only `CLUSTER_ADMIN` allows READ/WRITE/CREATE/AUDIT_READ/… without extra grants
- [ ] T036 [P] [US2] Add conformance tests in `crates/conformance` for SC-002: CLUSTER_ADMIN smoke create/write without extra grants; RolePut/edit on `admin`/`replication` → `BuiltinRoleImmutable`; custom RolePut on first-binary → `Slice7Required`; bound tenant SET/GET via implicit grant

### Implementation for User Story 2

- [ ] T037 [US2] Implement `Authorizer::authorize(principal_id, verb, resource{namespace_id?, container_id?}) -> Allow | Deny` in `crates/authz/src/permission.rs` (or `lib.rs`); Deny maps to protocol authorization error (`002`)
- [ ] T038 [US2] Encode builtins in `crates/authz` + call sites in `crates/tenancy`: `admin` exactly `{CLUSTER_ADMIN}` cluster scope; `replication` exactly `{REPLICATE}` node identity; `RolePut`/`RoleDelete`/rename of builtins → `BuiltinRoleImmutable`; custom name `admin`/`replication` → `RoleNameReserved`
- [ ] T039 [US2] Implement first-binary implicit tenant grant in `crates/authz/src/permission.rs`: namespace-bound principal that is not `admin` or `replication` gets `{READ, WRITE, CREATE, DROP, CONFIGURE}` **on that namespace only** (not a stored custom role); slice 7 replaces with explicit sets
- [ ] T040 [US2] Refuse custom RolePut / remaining-verb grants / tenant `AUDIT_READ` attach on first-binary profile with `Slice7Required` in `crates/tenancy` + `crates/authz` when feature `authz-custom` is off
- [ ] T041 [US2] Wire `REPLICATE` peer auth on `internode`/`replication` in `crates/internodes` / `crates/membership`: join secret + builtin `replication`; presenting replication identity on tenant handler → `ReplicationNotTenant`; MUST NOT be a tenant login
- [ ] T042 [US2] Ensure `CLUSTER_ADMIN` admits join, changes global config, and creates/writes containers in any namespace without separate READ/WRITE grants (integration with `011`/`007` call sites) via `authorize` implication
- [ ] T043 [P] [US2] Under `authz-custom` (slice 7 only): enable custom roles with optional `container_ids` narrowing for READ/WRITE/CONFIGURE (empty = all containers); CREATE/DROP namespace-wide; custom including `CLUSTER_ADMIN` gets implication — stub/feature-gate in `crates/authz` + `crates/tenancy` without requiring first-binary delivery
- [ ] T044 [P] [US2] Document that UIs (`09`) MUST call the same `Authorizer` (no private privilege model) in `crates/authz` module docs or feature note; no UI work in this feature

**Checkpoint**: Builtin immutability and implication hold; Redis smoke uses bound tenant + implicit grant; custom roles absent on first-binary profile.

---

## Phase 5: User Story 3 - Transport and keys (Priority: P1)

**Goal**: Consume `001` tls/plaintext (omit = startup error); optional mTLS mapping; envelope master→KEK→data key replacing `keyring_file`; master rotate = rewrap KEKs (0 table rewrites); encrypt opt-in; data-key rewrite not first-binary.

**Independent Test**: Omit transport (fail start); encrypt a container; steal volume snapshot (ciphertext); rotate master; restore with specified key. SC-003, SC-004, SC-005.

### Tests for User Story 3 ⚠️

- [ ] T045 [P] [US3] Add unit tests in `crates/crypto` for master wrap/unwrap KEK, data-key wrap under KEK, master rotate rewraps all `KekRecord`s with unchanged data keys (0 table rewrites), and `KeyUnresolvable{key_ref}`
- [ ] T046 [P] [US3] Add config validate tests for [contracts/fixtures/invalid/](contracts/fixtures/invalid/) `keyring-file` → `KeyringRemoved`, `master-key-inline` → `KeyMaterialForbidden`, and master mode/size errors
- [ ] T047 [P] [US3] Add conformance tests in `crates/conformance` for SC-004/SC-005: encrypted container snapshot unreadable without master; `keys rotate-master --new-file P` rewraps KEKs with 0 rewrites; restore with specified key unlocks (`013` seam)

### Implementation for User Story 3

- [ ] T048 [US3] Confirm/consume `001` transport validation in `crates/config` / `crates/node`: every entrypoint `tls {…}` or `plaintext;`; omitted → `TransportOmitted` (SC-003 already `001`); TLS-declared refuses plaintext with no silent fallback — no duplicate grammar in authz
- [ ] T049 [P] [US3] Implement optional mTLS in `crates/config` + `crates/node` + `crates/authz`: `tls { client_ca P; }` maps CN (else first DNS SAN) to principal login; default mTLS additional to password; `mtls_replace_password` makes cert sufficient; certs referenced never inlined (`KeyMaterialForbidden`) per [contracts/tls.md](contracts/tls.md)
- [ ] T050 [US3] Implement `EnvelopeAuthority` implementing `KeyAuthority` in `crates/crypto/src/key_authority.rs`: resolve master→KEK→data key into `Zeroizing<[u8;32]>`; cache unwrapped data keys only for hosted containers; delete interim `keyring_file` provider once wired
- [ ] T051 [US3] Add `keys { master_key_file P; }` and optional `create_master_if_absent;` in `crates/config`; reject `keys { keyring_file … }` → `KeyringRemoved`; if any container encrypts or directive set without create-if-absent → require 32-byte `0600` file
- [ ] T052 [US3] On namespace create in `crates/tenancy` / controlplane apply: generate 32-byte KEK, wrap under master, append `KekRecord` to cluster log
- [ ] T053 [US3] Bind `key_ref` on container (`CLUSTER_ADMIN` any namespace, first binary) via admin `KeysBind` in `crates/admin-proto` + `crates/crypto`; algorithms `aes-256-gcm` (default) and `chacha20-poly1305`; store wrapped data key on container definition (`003`)
- [ ] T054 [US3] Implement `KeysRotateMaster { new_file }` in `crates/crypto` + admin CLI `spacestorage keys rotate-master --new-file P`: rewrap every `KekRecord`, bump `kek_epoch`, data keys unchanged, 0 table rewrites
- [ ] T055 [US3] Implement `KeysRotateData` issue new version on container; old versions stay readable; invoking payload rewrite on first-binary → `DataKeyRewriteNotFirstBinary` (rewrite waits for `010`)
- [ ] T056 [US3] Wire restore with `--key REF` / `--master-key-file` seam to `013` in `crates/spacestorage` / durability path; wrong/missing → fail naming the reference; lost master without backup ⇒ encrypted unrestorable, unencrypted unaffected
- [ ] T057 [US3] Refuse node start when encrypting without readable master in `crates/node`; `KeysStatus` reports master present / KEK epochs with **no material** in `crates/admin-proto`
- [ ] T058 [P] [US3] Document master-key backup = copy the file in [quickstart.md](quickstart.md) validation path / operator note under `docs/` examples referencing `master_key_file`

**Checkpoint**: Envelope replaces keyring; master rotate SC-005; ciphertext without master SC-004; transport omit still fails via `001`.

---

## Phase 6: User Story 4 - Audit (Priority: P2)

**Goal**: Append-only cluster-log audit for join, key bind/rotate, failed auth, login rename, namespace rename; readable by `CLUSTER_ADMIN` in first binary; no key material in entries.

**Independent Test**: Join, key rotate, failed login; `CLUSTER_ADMIN` reads audit with principal, action, target, time. SC-006.

### Tests for User Story 4 ⚠️

- [ ] T059 [P] [US4] Add unit tests in `crates/authz/src/audit.rs` that required first-binary actions serialize with principal id (or nil), action, target, Hlc and never include key bytes
- [ ] T060 [P] [US4] Add conformance tests in `crates/conformance` for SC-006: failed Redis AUTH, membership join, key rotate appear in `spacestorage audit` readable by `CLUSTER_ADMIN`

### Implementation for User Story 4

- [ ] T061 [US4] Implement `AuditAppend` / `AuditList` in `crates/authz/src/audit.rs` with cluster-log durability (SoT is cluster log, not `08`); `08` MAY optionally export later — do not require channel on for first binary
- [ ] T062 [US4] Append first-binary required actions from [contracts/audit.md](contracts/audit.md): `auth.ok`, `auth.fail`, `membership.join|leave|replace|token`, `key.bind|rotate_master|rotate_data`, `principal.rename`, `namespace.rename` (rename executed in `007`; this feature appends)
- [ ] T063 [US4] Hook failed AUTH and successful admin/join-related AUTH to audit from `crates/authz` authenticator path; hook key bind/rotate from `crates/crypto` / admin keys ops; hook join events from `crates/membership`
- [ ] T064 [US4] Hook `principal.rename` audit from principal rename path and `namespace.rename` append from `crates/tenancy` rename call site
- [ ] T065 [US4] Gate `AuditList` to `CLUSTER_ADMIN` in first binary via `authorize(AUDIT_READ)` implication; tenant `AUDIT_READ` grant waits for `authz-custom` (slice 7 refuse without it)
- [ ] T066 [US4] Add `spacestorage audit` CLI table (time, principal, action, target) and `--output json` in `crates/spacestorage` + `AuditList` in `crates/admin-proto`
- [ ] T067 [P] [US4] Increment metrics in `crates/authz`: `spacestorage_auth_attempts_total{protocol,result}`, `spacestorage_authz_denied_total{verb}`, `spacestorage_audit_entries_total{action}`, `spacestorage_key_unwrap_total{result}`, `spacestorage_key_master_epoch` per [contracts/metrics.md](contracts/metrics.md); no login/token/key material in labels; exposition owned by `008`

**Checkpoint**: SC-006 green; audit readable by CLUSTER_ADMIN; metrics increment without leaking secrets.

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: Cross-story hardening, release-profile flags, quickstart validation

- [ ] T068 [P] Ensure `release-profile` first binary compiles principal store + envelope + audit append and keeps `authz-custom` off by default in `crates/release-profile`
- [ ] T069 [P] Run full [quickstart.md](quickstart.md) validation path (config invalid fixtures §0 through audit §6) against in-process node and fix any drift in fixtures under `specs/014-authz-keys/contracts/fixtures/`
- [ ] T070 Confirm PBKDF2/SCRAM on `spawn_blocking` when iterations are high and file reads via `tokio::fs` in `crates/authz` / `crates/crypto` (constitution II)
- [ ] T071 [P] Sweep logs/describe/admin status paths so key material, plaintext passwords, and bearer tokens never appear (`KeyMaterialForbidden` / zeroize drop)
- [ ] T072 Code cleanup: remove leftover dual-path reads of `users_file` / `keyring_file` / `admin.token_file` once seams are wired in handlers, crypto, and config
- [ ] T073 [P] Add slice-7 note tasks only as feature-gated stubs already present — do not implement LDAP/SSO, external KMS, data-key rewrite (`010`), or UI chrome (`09`) in this feature

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately
- **Foundational (Phase 2)**: Depends on Setup — **BLOCKS** all user stories
- **User Story 1 (Phase 3)**: Depends on Foundational — 🎯 MVP
- **User Story 2 (Phase 4)**: Depends on Foundational; practical integration with US1 Authenticator/Authorizer call sites (can start Authorizer types in parallel after T004–T009)
- **User Story 3 (Phase 5)**: Depends on Foundational (crypto primitives T011–T013); config/node transport can proceed in parallel with US1 after foundation
- **User Story 4 (Phase 6)**: Depends on Foundational audit type (T014); hooks need US1 auth fail path, US3 key rotate, and membership join
- **Polish (Phase 7)**: Depends on desired user stories being complete

### User Story Dependencies

- **US1 (P1)**: After Foundational — no dependency on US2–US4 for core auth/bind MVP
- **US2 (P1)**: After Foundational — Authorizer used by US1 session re-check; builtins/implication can land before Redis smoke completes
- **US3 (P1)**: After Foundational crypto files — independent of principal rename; master required when encrypting
- **US4 (P2)**: After Foundational; append hooks need US1 fail-auth + US3 rotate + membership join events

### Within Each User Story

- Tests (where listed) MUST be written and FAIL before implementation
- Models/types before services before handler/admin wiring
- Story complete before treating checkpoint as done

### Parallel Opportunities

- Phase 1: T003 parallel with T001–T002 after Cargo.toml exists
- Phase 2: T005–T009, T011–T014 marked [P] can proceed in parallel after T004 Verb exists (T005–T006 independent of Verb; T004||T005||T006||T008||T011||T014)
- US1 tests T015–T019 in parallel; handler wires T025||T026 after Authenticator (T021)
- US2 tests T035–T036 and slice-7 stubs T043–T044 in parallel with core Authorizer once T037 lands
- US3 tests T045–T047 and mTLS T049 / docs T058 in parallel with envelope core
- US4 tests T059–T060 and metrics T067 in parallel with append implementation

---

## Parallel Example: User Story 1

```bash
# Launch US1 tests together:
Task: "Unit tests SCRAM in crates/authz/src/scram.rs"
Task: "Config validate fixtures in crates/config"
Task: "Conformance SC-008 bootstrap in crates/conformance"
Task: "Conformance SC-001 bind/refuse in crates/conformance"
Task: "Conformance SC-007/SC-009 rename/password in crates/conformance"

# After Authenticator exists, wire handlers in parallel:
Task: "Wire handler-postgresql to authz::Authenticator"
Task: "Wire handler-redis AUTH to authz::Authenticator"
```

---

## Parallel Example: User Story 3

```bash
# Launch US3 tests together:
Task: "Unit envelope rewrap tests in crates/crypto"
Task: "Config KeyringRemoved / KeyMaterialForbidden fixtures"
Task: "Conformance SC-004/SC-005 in crates/conformance"

# Parallel docs/mTLS with envelope core:
Task: "Optional mTLS client_ca mapping"
Task: "Document master-key backup = copy file"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL — blocks all stories)
3. Complete Phase 3: User Story 1 (bootstrap admin, SCRAM/Redis bind, rename, generation re-check)
4. **STOP and VALIDATE**: Independent Test for US1 / SC-001, SC-007, SC-008, SC-009
5. Demo/smoke before keys and audit

### Incremental Delivery

1. Setup + Foundational → types and codes ready
2. US1 → authenticate/bind MVP
3. US2 → vocabulary + builtins + implicit tenant grant (needed for Redis smoke completeness)
4. US3 → envelope keys + transport/mTLS consume
5. US4 → audit SoT + CLI read
6. Polish → quickstart + release-profile

### Parallel Team Strategy

1. Team completes Setup + Foundational together
2. Once Foundational is done:
   - Developer A: US1 authenticator + handlers + admin principals
   - Developer B: US2 Authorizer + tenancy builtin immutability
   - Developer C: US3 EnvelopeAuthority + master file
3. US4 audit hooks after A/C event sources exist

---

## Notes

- [P] tasks = different files, no dependencies on incomplete tasks
- [Story] label maps task to US1–US4 for traceability
- First binary MUST NOT require custom roles, tenant `AUDIT_READ`, KMS, or data-key rewrite
- S3/WebDAV/ES unbound refuse applies when those handlers exist (slice 6); not a first-binary ship gate
- Verify listed tests fail before implementing
- Commit after each task or logical group
- Stop at any checkpoint to validate the story independently
