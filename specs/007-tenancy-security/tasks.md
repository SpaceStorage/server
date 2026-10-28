---
description: "Task list for namespaces, quotas, access policies, encryption attachment, and roles"
---

# Tasks: Namespaces, Quotas, Access Policies, Encryption Attachment, and Roles

**Input**: Design documents from `/specs/007-tenancy-security/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md), [data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: Requested in [plan.md](plan.md) Technical Context (`cargo test`; unit tests in `tenancy`; `crates/conformance` first-binary + `tenancy-quotas` suites; contract fixtures). Write failing tests first where listed below.

**Scope of this feature**: New library crate `crates/tenancy` (`spacestorage-tenancy`) interprets cluster-store tenancy records. Schemas/containers stay in namespace Raft (`06`). AuthN/verbs/KEK/audit stay in `14`. Private metrics/logs export waits for `08`. Hard quotas, custom roles, and access policies are Cargo feature `tenancy-quotas` (slice 7).

**Sibling crates** (wire seams; do not reimplement Raft/authz/types): `crates/controlplane`, `crates/internode`, `crates/catalog`, `crates/types`, `crates/exec`, `crates/node`, `crates/config`, `crates/admin-proto`, `crates/spacestorage`, `crates/protocol-pg`, `crates/protocol-redis`, `crates/release-profile`, `crates/conformance`.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: [US1], [US2], [US3] on user-story phase tasks only
- Every task includes an exact file path

## Path Conventions

- Core crate: `crates/tenancy/src/`
- Workspace root: `Cargo.toml`
- Integration: sibling crates under `crates/` as listed in [plan.md](plan.md)

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Workspace member, crate skeleton, Cargo feature inventory from [research.md](research.md) R1/R16

- [X] T001 Create `crates/tenancy/Cargo.toml` (package `spacestorage-tenancy`, edition 2024, MSRV-compatible with workspace) with deps `tokio`, `async-trait`, `serde`/`serde_json`, `bytes`, `tracing`, `uuid`, `parking_lot`, and `[features] default = []` plus `tenancy-quotas = []`
- [X] T002 Add `crates/tenancy` to workspace `[workspace.members]` in `Cargo.toml` and declare a path dependency from crates that will call it (`controlplane`, `exec`, `node`, `config`, `admin-proto`, `spacestorage`, `release-profile`, `conformance`) once those crates exist
- [X] T003 Create `crates/tenancy/src/lib.rs` that `mod`s `error`, `name`, `registry`, `roles`, `quotas`, `policy`, `usage`, `admit`, `encryption`, `ops` and exports a `Tenancy` / `NamespaceRegistry` handle per [plan.md](plan.md) Project Structure
- [X] T004 [P] Copy [contracts/fixtures/namespace-block.conf](contracts/fixtures/namespace-block.conf) into `docs/examples/namespace-block.conf` (append note: use with `016` one-node starter) and ensure invalid fixtures under `specs/007-tenancy-security/contracts/fixtures/invalid/` remain the validate cases from [quickstart.md](quickstart.md)

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Shared types, validation codes, name rules, and cluster-log op shapes every story uses. No user-story work until this phase is complete.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [X] T005 Implement validation / error types in `crates/tenancy/src/error.rs`: `NamespaceExists { name }`, `NamespaceNameInvalid { name }`, `NamespaceNotFound { name }`, `CascadeRequired { name, containers }`, `NotClusterAdmin`, `QuotaExceeded { quota, limit, usage }`, `QuotaUnitUnknown { unit }`, `QuotaNegative`, `Slice7Required { op }`, `ObservabilityRequired { op }`, `KeyMaterialForbidden`, `EncryptionScopeInvalid { mode, scope }`, `AccessDenied { verb, datatype, namespace }`, `ReplicationNotTenant` exactly as [data-model.md](data-model.md) Validation codes
- [X] T006 [P] Implement namespace name rules in `crates/tenancy/src/name.rs`: unique cluster-wide, case-sensitive, `^[A-Za-z][A-Za-z0-9_]*$`, length 1–63 chars; reject → `NamespaceNameInvalid`
- [X] T007 [P] Implement `NamespaceId` (UUID, immutable) and `NamespaceName` newtypes plus `NamespaceRecord` fields in `crates/tenancy/src/registry.rs`: `id`, `name`, `quotas` (empty = unlimited), `policies` (empty until slice 7), `private_metrics`/`private_logs` (default false), `group_id` as `GroupId::Namespace(id)`, `created_hlc`, `deleted` tombstone flag per [data-model.md](data-model.md) §2
- [X] T008 [P] Implement `QuotaSpec` in `crates/tenancy/src/quotas.rs` with `unit` ∈ {`bytes`,`objects`,`connections`,`ops_per_sec`}, `limit: u64` (`0` = reject consuming work immediately), optional `datatype` (`03` type name); document that usage for `bytes`/`objects` is logical (RF MUST NOT multiply)
- [X] T009 [P] Implement `Role`, `RoleBinding`, and `AccessPolicy` stub types in `crates/tenancy/src/roles.rs` and `crates/tenancy/src/policy.rs` per [data-model.md](data-model.md) §5–§7 (`scope_kind` `cluster`|`namespace`; bindings key `principal_id` UUID, not login name)
- [X] T010 Implement `TenancyOp` enum in `crates/tenancy/src/ops.rs` (or `registry.rs`): `NamespaceCreate { name }`, `NamespaceRename { id, new_name }`, `NamespaceDelete { id, cascade }`, `QuotaReplace { id, quotas }`, `PolicyReplace { id, policies }`, `RolePut { role }`, `RoleBindingPut { binding }`, `RoleBindingDelete { principal_id }` with serde for cluster-log bodies per [data-model.md](data-model.md) §9
- [ ] T011 Wire `controlplane` cluster-group append path so `TenancyOp` bodies commit only on cluster-voter majority in `crates/controlplane/` (replace opaque `006` roles blob with typed records; minority → `Minority { group: cluster }`); list/get MUST read cluster applied state only (SC-009)
- [ ] T012 Gate registry/quota/policy mutate ops on `CLUSTER_ADMIN` in `crates/tenancy/src/ops.rs` (or authz seam): non-`CLUSTER_ADMIN` → `NotClusterAdmin`; `CONFIGURE` MUST NOT grant registry/quota writes (FR-014)
- [X] T013 Add unit tests in `crates/tenancy/src/name.rs` (and/or `crates/tenancy/tests/name.rs`) covering charset/length rejects and uniqueness helpers used by create/rename

**Checkpoint**: `cargo test -p spacestorage-tenancy` compiles; name/error unit tests pass. User stories can start.

---

## Phase 3: User Story 1 - Create a namespace with quotas (Priority: P1) 🎯 MVP

**Goal**: `CLUSTER_ADMIN` creates/lists/renames/deletes namespaces in the cluster store; starter `acme`; first binary MUST NOT reject on unset quotas. Slice 7 (`tenancy-quotas`): hard logical quotas with best-effort hard reject, never hang, never serialize tenant writes on cluster primary.

**Independent Test**: First binary: `CLUSTER_ADMIN` creates two namespaces, create containers in each, list, rename one, delete an empty one; starter example exists; `NAMESPACE_ADMIN` create/delete/rename refused. Slice 7: fill one namespace to byte quota → reject; sibling unaffected; RF does not multiply usage; `NAMESPACE_ADMIN` quota change refused; concurrent near-cap writes never hang.

### Tests for User Story 1 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [X] T014 [P] [US1] Add unit tests in `crates/tenancy/src/registry.rs` (or `crates/tenancy/tests/registry.rs`) for create/rename unique index, id/`group_id` unchanged on rename, `NamespaceExists` on duplicate target, `CascadeRequired` when delete without cascade and containers present
- [ ] T015 [P] [US1] Add first-binary conformance tests in `crates/conformance/tests/tenancy_namespaces.rs`: starter `acme` from [namespace-block.conf](contracts/fixtures/namespace-block.conf); create/list/rename/delete empty; list without contacting namespace Raft; `NAMESPACE_ADMIN` create/delete/rename → `NotClusterAdmin`
- [ ] T016 [P] [US1] Add config validate tests that `specs/007-tenancy-security/contracts/fixtures/invalid/quota-negative.conf` → exit 2 `QuotaNegative` and `invalid/quotas-on-first-binary.conf` → exit 2 `Slice7Required` on first-binary profile in `crates/config/tests/tenancy_config.rs` (or `crates/conformance`)
- [ ] T017 [P] [US1] Add `tenancy-quotas` conformance tests in `crates/conformance/tests/tenancy_quotas.rs` (feature-gated): fill-to-cap `QuotaExceeded { quota, limit, usage }`; sibling namespace writable; RF increase leaves logical bytes/objects unchanged; concurrent overshoot never hangs / never waits on cluster primary; `NAMESPACE_ADMIN` `QuotaReplace` → `NotClusterAdmin`

### Implementation for User Story 1 (first binary)

- [X] T018 [US1] Implement `NamespaceCreate` apply in `crates/tenancy/src/registry.rs`: assign immutable UUID id, unique name index, empty quotas/policies, `group_id = GroupId::Namespace(id)`; after cluster commit ask `006` to start namespace Raft with voter set copied from cluster (R2)
- [X] T019 [US1] Implement `NamespaceRename` in `crates/tenancy/src/registry.rs`: swap unique name index only; id, quotas, bindings, `group_id` unchanged; old name → `NamespaceNotFound` for new binds; target in use → `NamespaceExists`
- [X] T020 [US1] Implement `NamespaceDelete` in `crates/tenancy/src/registry.rs`: `cascade=false` + any containers → `CascadeRequired`; cascade destroys namespace group (`CLUSTER_ADMIN` only); refuse-unless-cascade required in first binary
- [X] T021 [US1] Implement list/get/`NamespaceView` in `crates/tenancy/src/registry.rs` from cluster applied state only (containers count from namespace catalog if reachable else `unknown`); never require namespace Raft for list (SC-009)
- [ ] T022 [US1] Implement `cluster { starter_namespace acme; }` create-if-absent **by name** in `crates/config/` + bootstrap apply in `crates/node/`; omitted directive → no implicit tenant; after rename `acme`→`contoso`, later start MAY create a new empty `acme` (R9)
- [ ] T023 [US1] Expose admin ops `NamespaceList` / `NamespaceCreate` / `NamespaceRename` / `NamespaceDelete` / `NamespaceDescribe` in `crates/admin-proto/` and CLI `spacestorage namespaces`, `namespace create|rename|delete|describe` in `crates/spacestorage/` per [contracts/admin-cli.md](contracts/admin-cli.md) (exit 4 = `NotClusterAdmin` / `CascadeRequired`)
- [ ] T024 [US1] Map PostgreSQL `CREATE DATABASE` / `DROP DATABASE` / `ALTER DATABASE … RENAME` and session `-d <name>` bind to registry ops in `crates/protocol-pg/` per [contracts/protocol-mapping.md](contracts/protocol-mapping.md); unauthorized → protocol permission error
- [X] T025 [US1] Ensure first binary with quotas unset never rejects writes for quota reasons in `crates/tenancy/src/admit.rs` (no-op admit when no `QuotaSpec`); `QuotaReplace` without `tenancy-quotas` → `Slice7Required`

### Implementation for User Story 1 (slice 7 / `tenancy-quotas`)

- [X] T026 [US1] Implement in-memory `QuotaUsage` ledger in `crates/tenancy/src/usage.rs` keyed by `(namespace_id, unit, datatype)`; seed from catalog logical sizes; `fetch_add`/`fetch_sub` on admit; periodic reconcile; **no** usage in cluster Raft (R4)
- [ ] T027 [US1] Implement additive internodes `QuotaDelta { namespace, unit, delta }` in `crates/internode/` and apply path in `crates/tenancy/src/usage.rs` for best-effort cross-coordinator usage
- [X] T028 [US1] Implement `admit` check/increment/decrement in `crates/tenancy/src/admit.rs`: if `current >= limit` → `QuotaExceeded { quota, limit, usage }`; MUST NOT hang; MUST NOT wait on cluster primary; on data-path failure after admit, decrement; order: authz → tenancy admit → `005` query admission (stricter wins) per [contracts/admission.md](contracts/admission.md)
- [ ] T029 [US1] Implement `QuotaReplace` (feature `tenancy-quotas`) in `crates/tenancy/src/ops.rs`: only `CLUSTER_ADMIN`; lowering below usage accepted; limit `0` rejects consuming work immediately; wire `namespace { quota { … } }` parse/validate in `crates/config/` per [contracts/config-directives.md](contracts/config-directives.md)
- [ ] T030 [US1] Call `tenancy::admit` before `Scheduled` in `crates/exec/` and session connect/disconnect → `connections` unit in `crates/node/`; consuming units per [contracts/admission.md](contracts/admission.md); `ops_per_sec` as 1s token bucket when set (R6)
- [ ] T031 [US1] Add CLI `spacestorage quotas set <name> --bytes …` and admin `QuotaReplace` in `crates/spacestorage/` / `crates/admin-proto/` gated by `tenancy-quotas`; enable feature in `crates/release-profile/` for slice 7 only (first binary compiles without enforce)

**Checkpoint**: First-binary namespace CRUD+rename+starter works and is independently testable; with `tenancy-quotas`, hard reject suite passes.

---

## Phase 4: User Story 2 - Roles and access policies (Priority: P1)

**Goal**: Persist and honor cluster-level `admin` and `replication` in the cluster store (first binary). Slice 7: custom roles (opaque `14` verb sets) and per-type access policies; UIs get no extra rights.

**Independent Test**: First binary: `admin` acts across namespaces; `replication` authenticates on `internode`/`replication`; roles survive restart. Slice 7: namespace-scoped custom role with READ only; writes/cross-namespace/per-type deny behave as specified.

### Tests for User Story 2 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [X] T032 [P] [US2] Add unit tests in `crates/tenancy/src/roles.rs` for builtin bootstrap of `admin`/`replication`, `ReplicationNotTenant` when replication used on tenant protocol, binding requires exactly one `namespace_id` for non-admin
- [ ] T033 [P] [US2] Add conformance tests in `crates/conformance/tests/tenancy_roles.rs`: roles present after restart from cluster store (SC-008); first-binary admin token maps to `admin`; custom `RolePut` without `tenancy-quotas` → `Slice7Required`
- [ ] T034 [P] [US2] Add `tenancy-quotas` tests in `crates/conformance/tests/tenancy_policies.rs`: READ-only custom role cannot write; cross-namespace refuse; Log Stream write deny vs K/V Store allow; empty permission set authenticates but can do nothing else

### Implementation for User Story 2

- [X] T035 [P] [US2] Bootstrap builtin roles in `crates/tenancy/src/roles.rs`: `admin` (cluster scope; gate name until `14` supplies opaque `CLUSTER_ADMIN` set) and `replication` (cluster/node scope; `REPLICATE` / internodes identity) on first apply (R3)
- [ ] T036 [US2] Implement `RolePut` / `RoleBindingPut` / `RoleBindingDelete` apply in `crates/tenancy/src/roles.rs` storing rows in cluster log; bindings use `principal_id` so `14` login rename does not rewrite the table; map `001` admin token → `admin` and internodes/`replication` entrypoints → `replication` in `crates/node/`
- [ ] T037 [US2] Refuse unbound non-admin on tenant protocols and refuse `replication` as tenant in protocol seams `crates/protocol-pg/` / `crates/protocol-redis/` (`ReplicationNotTenant` / auth refuse) per [contracts/roles.md](contracts/roles.md) and [contracts/protocol-mapping.md](contracts/protocol-mapping.md)
- [X] T038 [US2] Implement `AccessPolicy` evaluation and `PolicyReplace` behind `tenancy-quotas` in `crates/tenancy/src/policy.rs`: only `CLUSTER_ADMIN` writes; missing policy ⇒ role verbs on all types; matching policy MAY further deny; call from admit path after role verbs ([contracts/access-policies.md](contracts/access-policies.md), [contracts/admission.md](contracts/admission.md))
- [ ] T039 [US2] Expose `RoleList` / `RoleBindingList` and CLI `spacestorage roles` in `crates/admin-proto/` and `crates/spacestorage/`; document that `09` UIs MUST use the same evaluation (FR-012) with no extra rights

**Checkpoint**: Builtin roles survive restart; slice-7 custom roles/policies independently testable.

---

## Phase 5: User Story 3 - Tenant-private telemetry and encryption attachment (Priority: P2)

**Goal**: First binary: attach encryption declaration (algorithm, key_ref, scope) on container definitions; never emit key material. Store `private_metrics`/`private_logs` flags; turning them `on` before `08` → `ObservabilityRequired` (export is out of scope here).

**Independent Test**: First binary: declare encryption on a container; describe shows algorithm + reference + scope, never key; memory-mode + `drives` refused. After `08` (not required here): namespace-only metrics vs global scrape.

### Tests for User Story 3 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [X] T040 [P] [US3] Add unit tests in `crates/tenancy/src/encryption.rs` for `KeyMaterialForbidden` on inline key bytes and `EncryptionScopeInvalid` for memory-mode + `drives`
- [ ] T041 [P] [US3] Add conformance tests in `crates/conformance/tests/tenancy_encryption.rs`: describe shows algorithm/key_ref/scope with 0 key bytes (SC-007); `invalid/encryption-key-inline.conf` → exit 2 `KeyMaterialForbidden`
- [ ] T042 [P] [US3] Add unit/config tests that `private_metrics on` / `private_logs on` → `ObservabilityRequired` until `08` in `crates/tenancy/src/registry.rs` / `crates/config/`

### Implementation for User Story 3

- [X] T043 [P] [US3] Implement `EncryptionDeclaration` validation/redaction in `crates/tenancy/src/encryption.rs`: algorithm `AES-256-GCM` (default) | `ChaCha20-Poly1305`; `key_ref` string never key bytes; scope `drives` | `drives_and_memory`; redact on describe/logs
- [ ] T044 [US3] Attach encryption fields on container definition in namespace Raft via `crates/catalog/` and/or `crates/types/` (`03` fields); persist and show in first binary; who may bind `key_ref` is `NAMESPACE_ADMIN` or `CLUSTER_ADMIN` for that namespace (container-option write, not registry write) per [contracts/encryption-declaration.md](contracts/encryption-declaration.md)
- [ ] T045 [US3] Surface encryption on container describe paths used by `spacestorage namespace describe` / admin views in `crates/admin-proto/` and `crates/spacestorage/` without ever logging key material (FR-011)
- [X] T046 [US3] Persist `private_metrics` / `private_logs` on `NamespaceRecord` in `crates/tenancy/src/registry.rs`; omitted/`off` allowed in first binary; `on` at validate → `ObservabilityRequired` (R11); do not implement export endpoints (owned by `08`)

**Checkpoint**: Encryption declarations safe to describe; private flags stored but not exported.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Metrics hooks, release-profile wiring, quickstart validation, docs

- [X] T047 [P] Increment reserved metrics in `crates/tenancy/src/usage.rs` / `admit.rs`: `spacestorage_namespace_usage_bytes`, `spacestorage_namespace_usage_objects`, `spacestorage_namespace_connections`, `spacestorage_quota_exceeded_total` with labels per [contracts/metrics.md](contracts/metrics.md) (exposition remains `08`; do not rename)
- [X] T048 Wire first-binary `tenancy` (registry + builtin roles + encryption attach) into `crates/release-profile/` without enabling `tenancy-quotas` until slice 7
- [ ] T049 [P] Run [quickstart.md](quickstart.md) validation path: `spacestorage validate` on invalid fixtures; one-node starter with namespace block; CRUD/rename/encryption describe; document slice-7 and `08` optional sections as deferred gates
- [ ] T050 [P] Update feature docs cross-links in `specs/007-tenancy-security/plan.md` status note only if needed after implementation (keep tasks.md as source of truth for remaining work); ensure `checklists/requirements.md` still matches shipped FR-013 first-binary subset

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Setup — **BLOCKS** all user stories
- **User Story 1 (Phase 3)**: After Foundational — first-binary namespace MVP; slice-7 quota tasks (T026–T031, T017) need `tenancy-quotas`
- **User Story 2 (Phase 4)**: After Foundational — can parallel US1 once registry exists for bindings; custom roles/policies need `tenancy-quotas`
- **User Story 3 (Phase 5)**: After Foundational — encryption attach can parallel US1/US2; private export waits for `08` (flags only here)
- **Polish (Phase 6)**: After desired user stories complete

### User Story Dependencies

- **User Story 1 (P1)**: No dependency on US2/US3 for namespace CRUD; quota admit is self-contained under `tenancy-quotas`
- **User Story 2 (P1)**: Needs cluster log + registry from foundational/US1 create to bind non-admin principals to a namespace; builtin roles bootstrap can proceed in parallel with US1 registry
- **User Story 3 (P2)**: Needs container definition seam (`03`/`catalog`); independent of hard quotas

### Within Each User Story

- Tests (listed) MUST be written and FAIL before implementation
- Models/types before apply/ops
- Ops before admin/CLI/protocol wiring
- First-binary subset before `tenancy-quotas` gated work

### Parallel Opportunities

- Phase 1: T004 parallel with T001–T003 after crate exists
- Phase 2: T006, T007, T008, T009 parallel after T005
- Phase 3 tests: T014–T017 parallel
- Phase 3 impl: T023/T024 can parallel once T018–T021 land
- Phase 4 tests: T032–T034 parallel; T035 parallel with US1 registry work
- Phase 5: T040–T042 tests parallel; T043 parallel with US1/US2
- After Foundational: different developers can own US1 / US2 / US3 concurrently

---

## Parallel Example: User Story 1

```bash
# Launch first-binary tests together:
Task: "Unit tests for registry rename/cascade in crates/tenancy/src/registry.rs"
Task: "Conformance tenancy_namespaces.rs in crates/conformance/tests/"
Task: "Config validate invalid quota fixtures in crates/config/tests/"

# After registry types exist, launch apply paths:
Task: "NamespaceCreate/Rename/Delete in crates/tenancy/src/registry.rs"
Task: "Admin CLI namespace commands in crates/spacestorage/"
Task: "PG CREATE/DROP/RENAME DATABASE mapping in crates/protocol-pg/"
```

---

## Parallel Example: User Story 2

```bash
Task: "Builtin role unit tests in crates/tenancy/src/roles.rs"
Task: "Conformance tenancy_roles.rs in crates/conformance/tests/"
Task: "Bootstrap admin/replication in crates/tenancy/src/roles.rs"
```

---

## Parallel Example: User Story 3

```bash
Task: "Encryption unit tests in crates/tenancy/src/encryption.rs"
Task: "Conformance tenancy_encryption.rs in crates/conformance/tests/"
Task: "EncryptionDeclaration validate/redact in crates/tenancy/src/encryption.rs"
```

---

## Implementation Strategy

### MVP First (User Story 1 first-binary only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL)
3. Complete Phase 3 first-binary tasks (T014–T015, T018–T025) — namespace CRUD+rename+starter
4. **STOP and VALIDATE**: Independent Test for US1 first binary (SC-001, SC-009, SC-010, SC-011)
5. For a **shippable first binary** per FR-013, also complete US2 builtins (T032–T033, T035–T037, T039) and US3 encryption (T040–T041, T043–T045) before claiming slices 1–5 done
6. Defer `tenancy-quotas` (T017, T026–T031, T034, T038) and private export (`08`) to later slices

### Incremental Delivery

1. Setup + Foundational → foundation ready
2. US1 first-binary namespaces → demo CRUD/rename
3. US2 builtin roles → restart-durable `admin`/`replication`
4. US3 encryption attach → SC-007 describe safety
5. Enable `tenancy-quotas` → US1 quota admit + US2 custom roles/policies
6. After `08` → private metrics/logs export using stored flags

### Parallel Team Strategy

1. Team completes Setup + Foundational together
2. Developer A: US1 registry + admin/CLI + PG mapping
3. Developer B: US2 roles/bindings (+ later policies)
4. Developer C: US3 encryption + private flags
5. Share `tenancy-quotas` feature work after first-binary MVP validates

---

## Notes

- [P] = different files, no incomplete-task dependencies
- [USn] maps to spec user stories for traceability
- First binary MUST NOT require hard quotas, custom/per-type policies, or private export (FR-013)
- Key material MUST NEVER appear in describe/config/logs from this feature (FR-011)
- Quota usage is AP/best-effort; caps are CP on cluster Raft (R4/R5)
- Commit after each task or logical group; stop at checkpoints to validate independently

---

## Phase 7: Convergence

Library seam for the security predicates the unchecked conformance files would have locked. Admin, CLI, protocol, and cluster-log callers are still absent, so those tasks stay open below. Earlier `[ ]` rows are unchanged.

- [X] T051 Gate namespace create/rename/delete, `QuotaReplace`, `RolePut`, and `PolicyReplace` on `CLUSTER_ADMIN` via `require_cluster_admin` in `crates/tenancy`; `NAMESPACE_ADMIN` and `CONFIGURE` return `NotClusterAdmin` (FR-014 / T012)
- [X] T052 Refuse `replication` on a tenant protocol with `ReplicationNotTenant`, and refuse an unbound non-admin, in `crates/tenancy/src/roles.rs` (`authorize_tenant_protocol`). A bound principal with an empty verb set may pass this check and still authorizes nothing in `policy::evaluate` (T034 library seam / T037 library half)
- [X] T053 Refuse `private_metrics on` / `private_logs on` with `ObservabilityRequired` in `NamespaceRecord::set_private_telemetry` (`crates/tenancy/src/registry.rs`); describe of `EncryptionDeclaration` carries `key_ref` and no key bytes (T042 / T046 validate, SC-007 library half)

- [ ] T054 Wire `TenancyOp` commit on cluster-voter majority in `crates/controlplane/` so list/get read applied state only (T011)
- [ ] T055 Validate `quota-negative.conf` → `QuotaNegative` and `quotas-on-first-binary.conf` → `Slice7Required` in `crates/config` (T016), and `cluster { starter_namespace }` create-if-absent in `crates/config/` + `crates/node/` (T022)
- [ ] T056 Expose namespace, quota, and role admin ops plus CLI `spacestorage namespaces|namespace|quotas|roles` in `crates/admin-proto/` and `crates/spacestorage/`, including encryption describe with zero key bytes (T023, T031, T039, T045)
- [ ] T057 Map PostgreSQL `CREATE/DROP/ALTER DATABASE` in `crates/handler-postgresql/` and call `authorize_tenant_protocol` from the PostgreSQL and Redis tenant paths (T024, T037 handler half)
- [ ] T058 Add internodes `QuotaDelta` in `crates/internode/` and call `tenancy::admit` before `Scheduled` in `crates/exec/` plus the connections unit on session connect in `crates/node/` (T027, T030)
- [ ] T059 Persist `RolePut` / bindings in the cluster log from `crates/tenancy/src/roles.rs` and attach encryption fields on the container definition in `crates/catalog/` or `crates/types/` (T036, T044)
- [ ] T060 Add node conformance `tenancy_namespaces.rs`, `tenancy_quotas.rs`, `tenancy_roles.rs`, `tenancy_policies.rs`, and `tenancy_encryption.rs` once T054–T059 exist (T015, T017, T033, T034, T041). Fill-to-cap `QuotaExceeded` already rejects in `QuotaUsage::admit` and must be driven through that harness, including sibling namespace, RF not multiplying usage, and no wait on the cluster primary
- [ ] T061 Run `specs/007-tenancy-security/quickstart.md` and align the plan status note (T049, T050)

## Phase 8: Convergence

PostgreSQL and Redis now call `admit_tenant_login` after the password check. The rest of Phase 7 stays open; this phase is the live remainder.

- [X] T062 Call `admit_tenant_login` after PostgreSQL SCRAM success (`crates/handler-postgresql/src/serve.rs`, `28000` / `42501`) and after Redis `AUTH` (`crates/handler-redis/src/commands.rs`). `demo` bound to a namespace still authenticates. `replication` and an empty namespace do not (T037 handler half)

- [X] T063 Wire `TenancyOp` commit on cluster-voter majority in `crates/controlplane/` so list/get read applied state only (T054 / T011)
- [X] T064 Validate `quota-negative.conf` → `QuotaNegative` and `quotas-on-first-binary.conf` → `Slice7Required` in `crates/config`, and `cluster { starter_namespace }` create-if-absent in `crates/config/` + `crates/node/` (T055 / T016 / T022)
- [X] T065 Expose namespace, quota, and role admin ops plus CLI `spacestorage namespaces|namespace|quotas|roles` in `crates/admin-proto/` and `crates/spacestorage/`, including encryption describe with zero key bytes (T056 / T023 / T031 / T039 / T045)
- [X] T066 Map PostgreSQL `CREATE/DROP/ALTER DATABASE` and session database bind onto registry ops in `crates/handler-postgresql/` (T057 remainder / T024). Cross-namespace open by a bound principal is refused; `admin` may use another name
- [X] T067 Add internodes `QuotaDelta` in `crates/internode/` and call `tenancy::admit` before `Scheduled` in `crates/exec/` plus the connections unit on session connect in `crates/node/` (T058 / T027 / T030)
- [X] T068 Persist `RolePut` / bindings in the cluster log from `crates/tenancy/src/roles.rs` and attach encryption fields on the container definition in `crates/catalog/` or `crates/types/` (T059 / T036 / T044)
- [X] T069 Add node conformance `tenancy_namespaces.rs`, `tenancy_quotas.rs`, `tenancy_roles.rs`, `tenancy_policies.rs`, and `tenancy_encryption.rs` once T063–T068 exist (T060). Fill-to-cap `QuotaExceeded` is driven through that harness, including sibling namespace, RF not multiplying usage, and no wait on the cluster primary
- [X] T070 Run `specs/007-tenancy-security/quickstart.md` and align the plan status note (T061 / T049 / T050)

## Phase 9: Convergence

Admin TCP ops, cluster-log `TenancyOp`, and library predicates from Phase 8 are in place. These items are still unmet in the current code.

- [X] T071 Make `spacestorage namespaces`, `namespace create|rename|delete|describe`, `quotas set`, and `roles` perform the admin call and return real results and exit codes (4 for `NotClusterAdmin`, `CascadeRequired`, and `QuotaExceeded`) per SC-001 and contracts/admin-cli.md (quickstart.md §1–§6) (partial)
- [X] T072 Reject `contracts/fixtures/invalid/encryption-key-inline.conf` from `spacestorage validate` with exit 2 `KeyMaterialForbidden` (the `container` block is currently `unknown_directive`) per FR-011 and quickstart.md §0 (partial)
- [X] T073 Refuse a memory-mode container whose encryption scope is `drives` with `EncryptionScopeInvalid` on container attach (`ContainerCatalog::create_from_definition` stores the declaration unchecked) per US3/AC2 (partial)
- [X] T074 Apply byte and object quotas only to the matching datatype, and to every type only when `datatype` is unset, so a different type does not consume a per-type quota per US1/AC8 and FR-002 (partial)
- [X] T075 Evaluate role verbs and access policies on the write path so a READ-only custom role cannot write and a per-type write deny is honored per US2/AC4, US2/AC6, SC-005, and FR-004 (partial)
- [X] T076 Admit `ops_per_sec` with a 1-second token bucket per plan R6 and contracts/quotas.md (contradicts)
- [X] T077 Publish and apply internodes `QuotaDelta` so a later consuming request on another coordinator is rejected once usage is at the cap, without waiting on the cluster primary, per US1/AC11 and plan R4 (partial)
- [X] T078 Seed quota usage from catalog logical size and reconcile it so data already stored still counts toward the cap after restart per plan R4 and SC-003 (partial)
- [X] T079 Decrement quota usage when the data path fails after a successful admit per contracts/admission.md (partial)
- [X] T080 Reject `private_metrics on` and `private_logs on` at config validate with `ObservabilityRequired` per plan R11 and contracts/config-directives.md (partial)
- [X] T081 Increment `spacestorage_namespace_usage_bytes`, `spacestorage_namespace_usage_objects`, `spacestorage_namespace_connections`, and `spacestorage_quota_exceeded_total` from the usage ledger per FR-009 and contracts/metrics.md (partial)

## Phase 10: Convergence

Phase 9 library and PostgreSQL paths are in place. These items are still unmet in the current code.

- [X] T082 Run role-verb evaluation, access-policy checks, and quota admit — including the `connections` unit on authenticated session bind — on Redis and the other tenant protocols in contracts/protocol-mapping.md, passing the authenticated principal into the query engine the way PostgreSQL does, per FR-003, US1/AC6, US2/AC4, SC-005, and contracts/admission.md (partial)
- [X] T083 Publish internodes `QuotaDelta` to every other coordinator's internodes address, including from a bootstrap node whose `cluster.seeds` list is empty, so a later consuming request on another coordinator is rejected once usage is at the cap, without waiting on the cluster primary, per US1/AC11 and plan R4 (partial)
- [X] T084 Subject UI console queries and container creates to the same role, access-policy, and quota admission as other clients, per FR-012 (partial)
- [X] T085 Emit `spacestorage_namespace_connections` and `spacestorage_quota_exceeded_total` labeled by `namespace` and `unit` from the usage ledger (the connections series is currently renamed, and quota rejects are one aggregate `*` series) per FR-009, plan R15, and contracts/metrics.md (partial)

## Phase 11: Convergence

Phase 10 protocol admission, quota fanout, UI admission, and metric names are in place. These items are still unmet in the current code.

- [X] T086 Commit every consuming unit of one request together in `crates/tenancy/src/admit.rs`: if a later unit returns `QuotaExceeded`, roll back earlier increments from that request and any positive `QuotaDelta` already published for them, so a rejected request does not leave local or peer usage higher, per FR-003 (partial)
- [X] T087 Publish a negative internodes `QuotaDelta` from `admit::release` on data-path failure and on connection close, and reconcile bytes and objects from catalog logical size on a repeating cadence (not only at control-plane start). Connection usage is not in the catalog seed and must travel as a delta, per plan R4 and R7 (partial)
- [X] T088 Refuse Cassandra `USE` and `CREATE KEYSPACE` that point a bound principal at another namespace. `USE` of a different keyspace is `AccessDenied` unless the principal is `admin`. `CREATE KEYSPACE` is `NamespaceCreate` and requires `CLUSTER_ADMIN`. Later statements run only in a name that exists in the cluster registry, per SC-004 and contracts/protocol-mapping.md (contradicts)

## Phase 12: Convergence

Phase 11 multi-unit rollback, negative release deltas, connection-close release, and the repeating catalog reconcile are in place. Cassandra `USE` refuses a cross-namespace bind. These items are still unmet in the current code.

- [X] T089 Commit Cassandra `CREATE KEYSPACE` as `TenancyOp::NamespaceCreate` on cluster-voter majority through the same `append_tenancy` path as PostgreSQL (namespace Raft start included), not `NamespaceRegistry::create`. Keep `CLUSTER_ADMIN` required. `USE` and later CQL run only in a name present in that cluster-applied registry (`NamespaceNotFound` otherwise). A bound principal's `USE` of another namespace stays `AccessDenied` unless `admin`, per FR-006, plan R2, data-model §9, and contracts/protocol-mapping.md (contradicts)
- [X] T090 Map ClickHouse `CREATE DATABASE` to `TenancyOp::NamespaceCreate` and `RENAME DATABASE` to `TenancyOp::NamespaceRename` on that same cluster-voter commit path (`CLUSTER_ADMIN` only). Do not treat `CREATE DATABASE` as `CREATE TABLE`, per contracts/protocol-mapping.md (missing)
- [X] T091 On the periodic catalog reconcile, set bytes and objects for every registry namespace to the catalog logical sum, including 0 when that namespace or datatype has no containers, and drop stale per-type counters. Leave `connections` as internodes deltas only, per plan R4 (partial)
- [X] T092 Do not admit `bytes` or `objects` for a delete (`LogicalRequest::Mutate`). A delete at the object cap must not return `QuotaExceeded`; object and byte usage must be able to fall to the catalog logical size, per contracts/admission.md (contradicts)

## Phase 13: Convergence

Phase 12 commits Cassandra `CREATE KEYSPACE` and ClickHouse `CREATE DATABASE` / `RENAME DATABASE` through `append_tenancy`, zeros empty namespaces on the periodic reconcile, and does not admit bytes or objects on delete. This item is still unmet in the current code.

- [X] T093 When tenancy is attached, refuse Cassandra data CQL and ClickHouse statements other than `CREATE DATABASE` / `RENAME DATABASE` unless the session keyspace or database was accepted against the cluster-applied registry (`NamespaceNotFound`). `USE` already checks before it changes the keyspace; `SELECT`, `INSERT`, `UPDATE`, `DELETE`, and `CREATE TABLE` still run in the handler default `demo` with no registry check, and ClickHouse hello never checks. Do not re-check the name on every later statement after a successful bind (in-flight sessions stay bound across rename). per contracts/protocol-mapping.md (partial)

## Phase 14: Convergence

Phase 13 refuses Cassandra data CQL and ClickHouse non-DDL until the session name has been accepted once, and leaves `CREATE KEYSPACE` / `CREATE DATABASE` / `RENAME DATABASE` on the admin path. The handshake still rejects the connection when the handler default `demo` is not in the registry, so those statements never run against starter `acme`.

- [X] T094 When tenancy is attached, do not fail Cassandra authentication or the ClickHouse native/HTTP handshake because the handler default `demo` is absent from the cluster-applied registry. Leave `registry_accepted` false so data CQL and non-DDL SQL still return `NamespaceNotFound`, and accept a name once when the client presents it (`USE`, or a `STARTUP`/hello keyspace or database that is registered). `CREATE KEYSPACE`, `CREATE DATABASE`, and `RENAME DATABASE` stay on the admin path and must remain reachable in that case. Do not re-check after a successful accept. Without tenancy, behavior unchanged. per contracts/protocol-mapping.md and cluster-registry.md (partial)

## Phase 15: Convergence

Phase 14 leaves the Cassandra and ClickHouse handshake up when `demo` is absent, and accepts a presented keyspace or database once. ClickHouse still takes the `connections` hold against the handler default before that accept, so a cap on the accepted database does not reject the session.

- [X] T095 When tenancy is attached, charge the ClickHouse `connections` quota to the registry namespace that was accepted, not the handler default. Native hello and HTTP `database=` call `bind_namespace_session` on `session.namespace` (`demo`) before `try_registry_accept_presented_database`, and `USE` in `crates/handler-clickhouse/src/sql.rs` never calls `admit_connection`. If `demo` is absent the handshake stays up with no hold while data then runs in the accepted database; if `demo` is present the hold is charged to `demo`. Take one hold for the accepted name (hello, HTTP `database=`, or `USE`). If no registered name is accepted, do not fail the handshake because `demo` is missing, and leave data statements on `NamespaceNotFound`. Cassandra `STARTUP` already accepts before auth. Without tenancy, unchanged. per US1/AC6, FR-003, and plan R7 (partial)

## Phase 16: Convergence

Phase 15 moves a successful ClickHouse `connections` hold onto the accepted database and leaves the handshake up when `demo` is missing. The hold is still taken on the handler default before that accept, and a failed move drops it.

- [X] T096 When tenancy is attached, keep the ClickHouse `connections` hold only on a registry namespace this session has accepted. Native hello (`crates/handler-clickhouse/src/native.rs`) and HTTP (`crates/handler-clickhouse/src/http.rs`) still call `try_bind_handler_default_connection` before `try_registry_accept_presented_database`: a `QuotaExceeded` on handler default `demo` refuses the session before a different presented database is charged, and if no name is accepted a hold remains on `demo` while data statements return `NamespaceNotFound`. `USE` in `crates/handler-clickhouse/src/sql.rs` sets `registry_accepted` and `reconcile_connection_quota_for_namespace` (`dispatch.rs`) drops the previous hold before the new `admit_connection` succeeds, so a `QuotaExceeded` on the new name leaves the session accepted with no hold and later statements are not rejected. Charge hello, HTTP `database=`, and `USE` to the accepted name (one hold; release the previous only after the new admit succeeds). If no registered name is accepted, leave the handshake up and take no hold. A `QuotaExceeded` on the namespace actually being accepted may still refuse the session. Without tenancy, unchanged. per US1/AC6, FR-003, SC-003, and plan R7 (partial)

## Phase 17: Convergence

`crates/handler-clickhouse/Cargo.toml` depends on `spacestorage-tenancy` with `features = ["tenancy-quotas"]` unconditionally. Cargo unifies that feature into every build that links the crate. `cargo test -p spacestorage-conformance --features first-binary` therefore compiles `role_put` and `admit` with slice-7 enforcement: `custom_role_put_without_slice7_is_refused` returns `Ok(Role)` instead of `Slice7Required`, and a connection quota on a namespace record is enforced. `spacestoraged --features first-binary` does not link ClickHouse and stays unlimited. The same line also turns enforcement on for `spacestoraged --features handlers-complete` without the node's `tenancy-quotas` feature. ClickHouse hello / HTTP `database=` / `USE` hold ordering from T096 is in place.

- [X] T097 Do not enable `spacestorage-tenancy/tenancy-quotas` from the normal dependency in `crates/handler-clickhouse/Cargo.toml`. A first-binary build, including `cargo test -p spacestorage-conformance --features first-binary`, must compile tenancy without that feature so custom `role_put` returns `Slice7Required` and `admit` / `admit_connection` do not enforce caps. Slice 7 still enables the feature through `spacestorage-node` / conformance `tenancy-quotas`. ClickHouse unit tests that set a connection quota may turn the feature on only as a dev-dependency of `handler-clickhouse`. per FR-013, FR-005, and plan R16 (contradicts)

## Phase 18: Convergence

T097 is in place: the normal `handler-clickhouse` dependency does not enable `tenancy-quotas`, and `cargo tree -p spacestorage-conformance --features first-binary -e features` shows zero `tenancy-quotas` edges. These items are still unmet in the current code.

- [X] T098 Charge `bytes` and `objects` for a PostgreSQL `INSERT` or `UPDATE` that `lower_sql` returns as `AdHocSql`. In `crates/query/src/engine.rs`, `request_usage` yields `(0, 0)` for that variant, so `admit` skips those units even when usage is already at the cap (`Bytes if bytes > 0`, `Objects if objects > 0`). Redis, Cassandra, ClickHouse, and S3 puts already go through `LogicalRequest::Object` and do count. Use the statement payload for bytes and one object per inserted row. Leave `DELETE` and reads at zero bytes and objects (`ops_per_sec` unchanged). A consuming insert that arrives at or above the cap must return `QuotaExceeded` naming the quota, the limit, and current usage. per FR-003, US1/AC5, US1/AC6, SC-003, and contracts/admission.md (partial)
- [X] T099 Refuse PostgreSQL `DROP DATABASE` when the namespace still has containers and cascade was not requested. `ControlPlaneNamespaces::drop_namespace` in `crates/node/src/handler/controlplane_namespaces.rs` always commits `TenancyOp::NamespaceDelete { cascade: false }` and never inspects the catalog. `tenancy_namespace_delete` in `crates/node/src/admin/mod.rs` returns `CascadeRequired` in that case. The protocol delete must return `CascadeRequired` the same way and must not remove the registry row. per the spec delete edge case, FR-013, and contracts/cluster-registry.md (contradicts)
