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

- [ ] T001 Create `crates/tenancy/Cargo.toml` (package `spacestorage-tenancy`, edition 2024, MSRV-compatible with workspace) with deps `tokio`, `async-trait`, `serde`/`serde_json`, `bytes`, `tracing`, `uuid`, `parking_lot`, and `[features] default = []` plus `tenancy-quotas = []`
- [ ] T002 Add `crates/tenancy` to workspace `[workspace.members]` in `Cargo.toml` and declare a path dependency from crates that will call it (`controlplane`, `exec`, `node`, `config`, `admin-proto`, `spacestorage`, `release-profile`, `conformance`) once those crates exist
- [ ] T003 Create `crates/tenancy/src/lib.rs` that `mod`s `error`, `name`, `registry`, `roles`, `quotas`, `policy`, `usage`, `admit`, `encryption`, `ops` and exports a `Tenancy` / `NamespaceRegistry` handle per [plan.md](plan.md) Project Structure
- [ ] T004 [P] Copy [contracts/fixtures/namespace-block.conf](contracts/fixtures/namespace-block.conf) into `docs/examples/namespace-block.conf` (append note: use with `016` one-node starter) and ensure invalid fixtures under `specs/007-tenancy-security/contracts/fixtures/invalid/` remain the validate cases from [quickstart.md](quickstart.md)

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Shared types, validation codes, name rules, and cluster-log op shapes every story uses. No user-story work until this phase is complete.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [ ] T005 Implement validation / error types in `crates/tenancy/src/error.rs`: `NamespaceExists { name }`, `NamespaceNameInvalid { name }`, `NamespaceNotFound { name }`, `CascadeRequired { name, containers }`, `NotClusterAdmin`, `QuotaExceeded { quota, limit, usage }`, `QuotaUnitUnknown { unit }`, `QuotaNegative`, `Slice7Required { op }`, `ObservabilityRequired { op }`, `KeyMaterialForbidden`, `EncryptionScopeInvalid { mode, scope }`, `AccessDenied { verb, datatype, namespace }`, `ReplicationNotTenant` exactly as [data-model.md](data-model.md) Validation codes
- [ ] T006 [P] Implement namespace name rules in `crates/tenancy/src/name.rs`: unique cluster-wide, case-sensitive, `^[A-Za-z][A-Za-z0-9_]*$`, length 1–63 chars; reject → `NamespaceNameInvalid`
- [ ] T007 [P] Implement `NamespaceId` (UUID, immutable) and `NamespaceName` newtypes plus `NamespaceRecord` fields in `crates/tenancy/src/registry.rs`: `id`, `name`, `quotas` (empty = unlimited), `policies` (empty until slice 7), `private_metrics`/`private_logs` (default false), `group_id` as `GroupId::Namespace(id)`, `created_hlc`, `deleted` tombstone flag per [data-model.md](data-model.md) §2
- [ ] T008 [P] Implement `QuotaSpec` in `crates/tenancy/src/quotas.rs` with `unit` ∈ {`bytes`,`objects`,`connections`,`ops_per_sec`}, `limit: u64` (`0` = reject consuming work immediately), optional `datatype` (`03` type name); document that usage for `bytes`/`objects` is logical (RF MUST NOT multiply)
- [ ] T009 [P] Implement `Role`, `RoleBinding`, and `AccessPolicy` stub types in `crates/tenancy/src/roles.rs` and `crates/tenancy/src/policy.rs` per [data-model.md](data-model.md) §5–§7 (`scope_kind` `cluster`|`namespace`; bindings key `principal_id` UUID, not login name)
- [ ] T010 Implement `TenancyOp` enum in `crates/tenancy/src/ops.rs` (or `registry.rs`): `NamespaceCreate { name }`, `NamespaceRename { id, new_name }`, `NamespaceDelete { id, cascade }`, `QuotaReplace { id, quotas }`, `PolicyReplace { id, policies }`, `RolePut { role }`, `RoleBindingPut { binding }`, `RoleBindingDelete { principal_id }` with serde for cluster-log bodies per [data-model.md](data-model.md) §9
- [ ] T011 Wire `controlplane` cluster-group append path so `TenancyOp` bodies commit only on cluster-voter majority in `crates/controlplane/` (replace opaque `006` roles blob with typed records; minority → `Minority { group: cluster }`); list/get MUST read cluster applied state only (SC-009)
- [ ] T012 Gate registry/quota/policy mutate ops on `CLUSTER_ADMIN` in `crates/tenancy/src/ops.rs` (or authz seam): non-`CLUSTER_ADMIN` → `NotClusterAdmin`; `CONFIGURE` MUST NOT grant registry/quota writes (FR-014)
- [ ] T013 Add unit tests in `crates/tenancy/src/name.rs` (and/or `crates/tenancy/tests/name.rs`) covering charset/length rejects and uniqueness helpers used by create/rename

**Checkpoint**: `cargo test -p spacestorage-tenancy` compiles; name/error unit tests pass. User stories can start.

---

## Phase 3: User Story 1 - Create a namespace with quotas (Priority: P1) 🎯 MVP

**Goal**: `CLUSTER_ADMIN` creates/lists/renames/deletes namespaces in the cluster store; starter `acme`; first binary MUST NOT reject on unset quotas. Slice 7 (`tenancy-quotas`): hard logical quotas with best-effort hard reject, never hang, never serialize tenant writes on cluster primary.

**Independent Test**: First binary: `CLUSTER_ADMIN` creates two namespaces, create containers in each, list, rename one, delete an empty one; starter example exists; `NAMESPACE_ADMIN` create/delete/rename refused. Slice 7: fill one namespace to byte quota → reject; sibling unaffected; RF does not multiply usage; `NAMESPACE_ADMIN` quota change refused; concurrent near-cap writes never hang.

### Tests for User Story 1 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T014 [P] [US1] Add unit tests in `crates/tenancy/src/registry.rs` (or `crates/tenancy/tests/registry.rs`) for create/rename unique index, id/`group_id` unchanged on rename, `NamespaceExists` on duplicate target, `CascadeRequired` when delete without cascade and containers present
- [ ] T015 [P] [US1] Add first-binary conformance tests in `crates/conformance/tests/tenancy_namespaces.rs`: starter `acme` from [namespace-block.conf](contracts/fixtures/namespace-block.conf); create/list/rename/delete empty; list without contacting namespace Raft; `NAMESPACE_ADMIN` create/delete/rename → `NotClusterAdmin`
- [ ] T016 [P] [US1] Add config validate tests that `specs/007-tenancy-security/contracts/fixtures/invalid/quota-negative.conf` → exit 2 `QuotaNegative` and `invalid/quotas-on-first-binary.conf` → exit 2 `Slice7Required` on first-binary profile in `crates/config/tests/tenancy_config.rs` (or `crates/conformance`)
- [ ] T017 [P] [US1] Add `tenancy-quotas` conformance tests in `crates/conformance/tests/tenancy_quotas.rs` (feature-gated): fill-to-cap `QuotaExceeded { quota, limit, usage }`; sibling namespace writable; RF increase leaves logical bytes/objects unchanged; concurrent overshoot never hangs / never waits on cluster primary; `NAMESPACE_ADMIN` `QuotaReplace` → `NotClusterAdmin`

### Implementation for User Story 1 (first binary)

- [ ] T018 [US1] Implement `NamespaceCreate` apply in `crates/tenancy/src/registry.rs`: assign immutable UUID id, unique name index, empty quotas/policies, `group_id = GroupId::Namespace(id)`; after cluster commit ask `006` to start namespace Raft with voter set copied from cluster (R2)
- [ ] T019 [US1] Implement `NamespaceRename` in `crates/tenancy/src/registry.rs`: swap unique name index only; id, quotas, bindings, `group_id` unchanged; old name → `NamespaceNotFound` for new binds; target in use → `NamespaceExists`
- [ ] T020 [US1] Implement `NamespaceDelete` in `crates/tenancy/src/registry.rs`: `cascade=false` + any containers → `CascadeRequired`; cascade destroys namespace group (`CLUSTER_ADMIN` only); refuse-unless-cascade required in first binary
- [ ] T021 [US1] Implement list/get/`NamespaceView` in `crates/tenancy/src/registry.rs` from cluster applied state only (containers count from namespace catalog if reachable else `unknown`); never require namespace Raft for list (SC-009)
- [ ] T022 [US1] Implement `cluster { starter_namespace acme; }` create-if-absent **by name** in `crates/config/` + bootstrap apply in `crates/node/`; omitted directive → no implicit tenant; after rename `acme`→`contoso`, later start MAY create a new empty `acme` (R9)
- [ ] T023 [US1] Expose admin ops `NamespaceList` / `NamespaceCreate` / `NamespaceRename` / `NamespaceDelete` / `NamespaceDescribe` in `crates/admin-proto/` and CLI `spacestorage namespaces`, `namespace create|rename|delete|describe` in `crates/spacestorage/` per [contracts/admin-cli.md](contracts/admin-cli.md) (exit 4 = `NotClusterAdmin` / `CascadeRequired`)
- [ ] T024 [US1] Map PostgreSQL `CREATE DATABASE` / `DROP DATABASE` / `ALTER DATABASE … RENAME` and session `-d <name>` bind to registry ops in `crates/protocol-pg/` per [contracts/protocol-mapping.md](contracts/protocol-mapping.md); unauthorized → protocol permission error
- [ ] T025 [US1] Ensure first binary with quotas unset never rejects writes for quota reasons in `crates/tenancy/src/admit.rs` (no-op admit when no `QuotaSpec`); `QuotaReplace` without `tenancy-quotas` → `Slice7Required`

### Implementation for User Story 1 (slice 7 / `tenancy-quotas`)

- [ ] T026 [US1] Implement in-memory `QuotaUsage` ledger in `crates/tenancy/src/usage.rs` keyed by `(namespace_id, unit, datatype)`; seed from catalog logical sizes; `fetch_add`/`fetch_sub` on admit; periodic reconcile; **no** usage in cluster Raft (R4)
- [ ] T027 [US1] Implement additive internodes `QuotaDelta { namespace, unit, delta }` in `crates/internode/` and apply path in `crates/tenancy/src/usage.rs` for best-effort cross-coordinator usage
- [ ] T028 [US1] Implement `admit` check/increment/decrement in `crates/tenancy/src/admit.rs`: if `current >= limit` → `QuotaExceeded { quota, limit, usage }`; MUST NOT hang; MUST NOT wait on cluster primary; on data-path failure after admit, decrement; order: authz → tenancy admit → `005` query admission (stricter wins) per [contracts/admission.md](contracts/admission.md)
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

- [ ] T032 [P] [US2] Add unit tests in `crates/tenancy/src/roles.rs` for builtin bootstrap of `admin`/`replication`, `ReplicationNotTenant` when replication used on tenant protocol, binding requires exactly one `namespace_id` for non-admin
- [ ] T033 [P] [US2] Add conformance tests in `crates/conformance/tests/tenancy_roles.rs`: roles present after restart from cluster store (SC-008); first-binary admin token maps to `admin`; custom `RolePut` without `tenancy-quotas` → `Slice7Required`
- [ ] T034 [P] [US2] Add `tenancy-quotas` tests in `crates/conformance/tests/tenancy_policies.rs`: READ-only custom role cannot write; cross-namespace refuse; Log Stream write deny vs K/V Store allow; empty permission set authenticates but can do nothing else

### Implementation for User Story 2

- [ ] T035 [P] [US2] Bootstrap builtin roles in `crates/tenancy/src/roles.rs`: `admin` (cluster scope; gate name until `14` supplies opaque `CLUSTER_ADMIN` set) and `replication` (cluster/node scope; `REPLICATE` / internodes identity) on first apply (R3)
- [ ] T036 [US2] Implement `RolePut` / `RoleBindingPut` / `RoleBindingDelete` apply in `crates/tenancy/src/roles.rs` storing rows in cluster log; bindings use `principal_id` so `14` login rename does not rewrite the table; map `001` admin token → `admin` and internodes/`replication` entrypoints → `replication` in `crates/node/`
- [ ] T037 [US2] Refuse unbound non-admin on tenant protocols and refuse `replication` as tenant in protocol seams `crates/protocol-pg/` / `crates/protocol-redis/` (`ReplicationNotTenant` / auth refuse) per [contracts/roles.md](contracts/roles.md) and [contracts/protocol-mapping.md](contracts/protocol-mapping.md)
- [ ] T038 [US2] Implement `AccessPolicy` evaluation and `PolicyReplace` behind `tenancy-quotas` in `crates/tenancy/src/policy.rs`: only `CLUSTER_ADMIN` writes; missing policy ⇒ role verbs on all types; matching policy MAY further deny; call from admit path after role verbs ([contracts/access-policies.md](contracts/access-policies.md), [contracts/admission.md](contracts/admission.md))
- [ ] T039 [US2] Expose `RoleList` / `RoleBindingList` and CLI `spacestorage roles` in `crates/admin-proto/` and `crates/spacestorage/`; document that `09` UIs MUST use the same evaluation (FR-012) with no extra rights

**Checkpoint**: Builtin roles survive restart; slice-7 custom roles/policies independently testable.

---

## Phase 5: User Story 3 - Tenant-private telemetry and encryption attachment (Priority: P2)

**Goal**: First binary: attach encryption declaration (algorithm, key_ref, scope) on container definitions; never emit key material. Store `private_metrics`/`private_logs` flags; turning them `on` before `08` → `ObservabilityRequired` (export is out of scope here).

**Independent Test**: First binary: declare encryption on a container; describe shows algorithm + reference + scope, never key; memory-mode + `drives` refused. After `08` (not required here): namespace-only metrics vs global scrape.

### Tests for User Story 3 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T040 [P] [US3] Add unit tests in `crates/tenancy/src/encryption.rs` for `KeyMaterialForbidden` on inline key bytes and `EncryptionScopeInvalid` for memory-mode + `drives`
- [ ] T041 [P] [US3] Add conformance tests in `crates/conformance/tests/tenancy_encryption.rs`: describe shows algorithm/key_ref/scope with 0 key bytes (SC-007); `invalid/encryption-key-inline.conf` → exit 2 `KeyMaterialForbidden`
- [ ] T042 [P] [US3] Add unit/config tests that `private_metrics on` / `private_logs on` → `ObservabilityRequired` until `08` in `crates/tenancy/src/registry.rs` / `crates/config/`

### Implementation for User Story 3

- [ ] T043 [P] [US3] Implement `EncryptionDeclaration` validation/redaction in `crates/tenancy/src/encryption.rs`: algorithm `AES-256-GCM` (default) | `ChaCha20-Poly1305`; `key_ref` string never key bytes; scope `drives` | `drives_and_memory`; redact on describe/logs
- [ ] T044 [US3] Attach encryption fields on container definition in namespace Raft via `crates/catalog/` and/or `crates/types/` (`03` fields); persist and show in first binary; who may bind `key_ref` is `NAMESPACE_ADMIN` or `CLUSTER_ADMIN` for that namespace (container-option write, not registry write) per [contracts/encryption-declaration.md](contracts/encryption-declaration.md)
- [ ] T045 [US3] Surface encryption on container describe paths used by `spacestorage namespace describe` / admin views in `crates/admin-proto/` and `crates/spacestorage/` without ever logging key material (FR-011)
- [ ] T046 [US3] Persist `private_metrics` / `private_logs` on `NamespaceRecord` in `crates/tenancy/src/registry.rs`; omitted/`off` allowed in first binary; `on` at validate → `ObservabilityRequired` (R11); do not implement export endpoints (owned by `08`)

**Checkpoint**: Encryption declarations safe to describe; private flags stored but not exported.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Metrics hooks, release-profile wiring, quickstart validation, docs

- [ ] T047 [P] Increment reserved metrics in `crates/tenancy/src/usage.rs` / `admit.rs`: `spacestorage_namespace_usage_bytes`, `spacestorage_namespace_usage_objects`, `spacestorage_namespace_connections`, `spacestorage_quota_exceeded_total` with labels per [contracts/metrics.md](contracts/metrics.md) (exposition remains `08`; do not rename)
- [ ] T048 Wire first-binary `tenancy` (registry + builtin roles + encryption attach) into `crates/release-profile/` without enabling `tenancy-quotas` until slice 7
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
