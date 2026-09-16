# Feature Specification: Namespaces, Quotas, Access Policies, Encryption Attachment, and Roles

**Feature Branch**: `007-tenancy-security`

**Created**: 2026-09-14

**Updated**: 2026-09-16

**Status**: Draft

**Input**: User description: "Read .specify/intent/07-tenancy-security.md and specify this feature." — multi-tenant namespaces with schema and data; quotas (bytes, object/row counts, connections, optional operation counts) rejected at the limit; access policies per data type; per-namespace metrics/logs vs global; encryption declared per container; roles admin / replication / custom stored in cluster-level controller storage. AuthN, permission verbs, keyring, audit are `14`.

## Clarifications

### Session 2026-09-16

- Q: Which parts of namespaces, quotas, roles, encryption flags, and private tenant metrics must already work in the first shippable binary, and which wait for later slices? → A: First binary: create/list/delete namespaces (plus a documented starter), cluster-level `admin` and `replication` roles, and per-container encryption declarations. Hard quotas and custom/per-type access policies wait for slice 7. Private tenant metrics/logs wait for observability (`08`).
- Q: Where should namespace records, quotas, access policies, and role bindings live relative to cluster-level vs namespace-level controller storage? → A: Cluster store holds the namespace registry, quotas, access policies, and role bindings. Namespace Raft holds schemas and container definitions only.
- Q: When data is replicated, should byte and row/object quotas count the tenant’s data once, or count every replica copy? → A: Logical size only. Replica factor does not multiply bytes or row/object counts.
- Q: Who is allowed to create or delete namespaces and to set or raise quotas? → A: Only `CLUSTER_ADMIN` may create/delete namespaces and set or raise quotas (including cascade delete). `NAMESPACE_ADMIN` cannot change quotas.
- Q: If several coordinators accept writes at the same time, may stored bytes or rows briefly go past the quota, or must every consuming request serialize through the cluster primary so usage never exceeds the limit? → A: Best-effort hard reject: in-flight requests MAY briefly exceed; later consuming requests reject; never hang.
- Q: Must namespace names (and principal login names) be changeable? → A: Yes. Namespace **id** is immutable; **name** is a unique, renameable label (`CLUSTER_ADMIN`). Principal **id** is immutable; **login name** is a unique, renameable label (`14`). Bindings and Raft groups follow ids, not names.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Create a namespace with quotas (Priority: P1)

A **cluster admin** (`CLUSTER_ADMIN`, `14`) creates a tenant namespace. The **namespace registry** (name, quotas, access policies) is written to **cluster-level controller storage**. Schemas and container definitions for that namespace live in that namespace's Raft group (`06`), not in the cluster store. In the **complete product** (slice 7) that admin sets quotas: bytes stored, object/row counts, connections, optionally operation counts, optionally per data type. Byte and row/object usage is **logical** (one copy of the tenant's data). Replica factor MUST NOT multiply those counters. When a tenant exceeds a quota, the offending request is **rejected** with an error naming the quota; it does not hang. Enforcement is **best-effort hard reject**: in-flight work on several coordinators MAY briefly exceed the limit; later consuming requests MUST reject. Quota accounting MUST NOT serialize tenant writes through the cluster primary. CPU hard isolation is not offered. A `NAMESPACE_ADMIN` MAY read usage and MUST NOT create or delete namespaces or set, raise, or lower quotas.

The **first shippable binary** (`16` slices 1–5) MUST create, list, **rename**, and delete namespaces, including a documented starter namespace in examples. It MUST NOT require hard quota enforcement: omitted quotas MUST NOT reject writes for quota reasons.

**Why this priority**: Tenancy is a constitution principle; without namespaces there is nowhere to put containers. Quotas are the isolation/billing lever but are not needed for protocol smoke.

**Independent Test**: First binary: `CLUSTER_ADMIN` creates two namespaces, create containers in each, list, rename one, delete an empty one; confirm the starter example namespace exists; a `NAMESPACE_ADMIN` create/delete/rename is refused. Slice 7: fill one namespace to its byte quota, confirm reject, confirm the other namespace is unaffected; raise replica factor and confirm usage does not jump by RF; `NAMESPACE_ADMIN` quota change is refused; concurrent writes near the cap never hang and subsequent requests reject.

**Acceptance Scenarios**:

1. **Given** no namespace `acme` (first binary), **When** `CLUSTER_ADMIN` creates it, **Then** `acme` exists in the cluster store, is empty of containers, can receive schemas and data into its namespace Raft, and a cluster-admin list of namespaces includes `acme` without reading that namespace Raft.
2. **Given** first binary with quotas unset, **When** a tenant writes, **Then** the write is not rejected for quota reasons.
3. **Given** a `NAMESPACE_ADMIN` for `acme` (first binary), **When** they create, delete, cascade-delete, or rename a namespace, **Then** the attempt is refused.
4. **Given** namespace `acme` with containers (first binary), **When** `CLUSTER_ADMIN` renames it to `contoso`, **Then** the id and namespace Raft group are unchanged, new sessions bind as `contoso`, the name `acme` is free (or `NamespaceNotFound` until reused), and a duplicate target name is refused.
5. **Given** `acme` with a byte quota Q (slice 7), **When** writes would exceed Q, **Then** they are rejected naming the quota and the current usage; already stored data remains; the request MUST NOT hang.
6. **Given** quotas on connections and row counts (slice 7), **When** those limits are hit, **Then** the extra connection or insert is rejected naming that quota.
7. **Given** two namespaces (slice 7), **When** one is over quota, **Then** the other continues to accept work under its own quotas.
8. **Given** a per-type quota (slice 7; for example object storage bytes), **When** a different type is written, **Then** that write does not consume the object-storage quota.
9. **Given** a container at replica factor RF>1 (slice 7), **When** usage is computed for byte or row/object quotas, **Then** it equals logical size (one copy), not RF×logical size.
10. **Given** a `NAMESPACE_ADMIN` for `acme` (slice 7), **When** they set, raise, or lower a quota, **Then** the attempt is refused; `CLUSTER_ADMIN` may still change that quota.
11. **Given** several coordinators writing near the cap (slice 7), **When** in-flight requests overlap, **Then** usage MAY briefly exceed Q; **When** a later consuming request arrives, **Then** it is rejected; **When** those overlapping writes complete, **Then** none waited on the cluster primary solely for quota accounting.

---

### User Story 2 - Roles and access policies (Priority: P1)

The cluster has roles `admin`, `replication`, and `custom` with specified permissions. Custom roles are sets of the permission vocabulary owned by `14`. A tenant admin receives a custom role scoped to one namespace. Non-admin principals are bound to exactly one namespace. Replication authenticates nodes, not tenants. Policies may differ by data type inside a namespace.

The **first binary** MUST persist and honor cluster-level `admin` and `replication` roles in the **cluster store**. **Custom** roles and per-data-type access policies wait for **slice 7** (with the full `14` vocabulary) and also live in the cluster store, not in namespace Raft. One-namespace binding for non-admin principals remains `14` and is already required for first-binary protocol smoke.

**Why this priority**: Three role names were previously unimplementable without a permission vocabulary; this feature stores and attaches them.

**Independent Test**: First binary: `admin` acts across namespaces; `replication` authenticates on `internode`/`replication`; roles survive restart. Slice 7: create a namespace-scoped custom role with READ only; attempt writes, cross-namespace access, and a per-type write deny.

**Acceptance Scenarios**:

1. **Given** the `admin` role (first binary), **When** it acts, **Then** it may operate across namespaces (subject to `14` CLUSTER_ADMIN).
2. **Given** the `replication` role (first binary), **When** it is used on `internode`/`replication` entrypoints, **Then** it is accepted as a node identity, not as a tenant.
3. **Given** role records (first binary), **When** they are read after restart, **Then** they are still in cluster-level controller storage (`06`), not in a namespace Raft group.
4. **Given** a custom role with `READ` only on namespace `acme` (slice 7), **When** it writes or opens `otherns`, **Then** those attempts are refused.
5. **Given** a non-admin principal with no namespace binding, **When** it authenticates on a tenant protocol, **Then** authentication is refused (`02`/`14`).
6. **Given** an access policy that forbids writes to a `Log Stream` in `acme` but allows writes to `K/V Store` (slice 7), **When** each write is attempted, **Then** only the allowed type succeeds.

---

### User Story 3 - Tenant-private telemetry and encryption attachment (Priority: P2)

A container may declare encryption at rest (algorithm and key **reference**, scope); unwrap and key authority are `14`. This declaration is required in the **first binary**.

A tenant may expose metrics only for its namespace (personal endpoint or OTel push) and send logs to its own sink, distinct from global admin streams. `08` owns series and sinks; this feature owns the tenancy boundary. Private tenant metrics/logs wait for **observability (`08`)**, not the first binary and not slice 7.

**Why this priority**: Encryption flags are part of the first-binary master-key path; telemetry isolation is a tenant product surface that must not leak keys and must not block smoke.

**Independent Test**: First binary: declare encryption on a container and confirm descriptions show algorithm and reference, never the key. After `08`: enable namespace-only metrics; confirm global `/metrics` still has global series.

**Acceptance Scenarios**:

1. **Given** a container with encryption declared (first binary), **When** it is described, **Then** algorithm, key reference, and scope appear and key material does not.
2. **Given** a memory-mode container (first binary), **When** encryption scope `drives` is requested, **Then** it is refused (`03`).
3. **Given** namespace `acme` with private metrics enabled (`08`), **When** a tenant scrapes its endpoint (or receives OTel push), **Then** only `acme` series appear.
4. **Given** the same cluster (`08`), **When** an admin scrapes global `/metrics`, **Then** all namespaces' series they are allowed to see appear.
5. **Given** private logging enabled for `acme` (`08`), **When** tenant operations occur, **Then** those logs can be sent to the tenant sink without appearing only in the global stream (they MAY also be in global for the admin).

---

### Edge Cases

- Quota of zero (slice 7): namespace exists but data-plane writes/connections that consume that quota are rejected immediately.
- Lowering a quota below current usage (slice 7): accepted; new consuming requests reject until usage falls (same spirit as buffer lower in `01`). In-flight work already admitted MAY complete and briefly leave usage further above the new cap.
- Concurrent consuming requests (slice 7): MAY briefly exceed; MUST NOT hang; MUST NOT wait on cluster-primary serialization for quota. A request that arrives when usage is already at or above the cap MUST be rejected.
- Cluster-majority loss (slice 7): data-plane coordinators keep using last known caps (best-effort); they MUST NOT hang waiting for cluster metadata. This is not fail-closed freeze and not a promise of zero overshoot.
- First binary with quotas unset: writes and connections MUST NOT be rejected for quota reasons.
- Changing replica factor (`04`) MUST NOT by itself cause a byte or row/object quota reject. Physical bytes across replicas MAY appear as operator metrics in `08` and MUST NOT be a tenant quota unit.
- Deleting a namespace that still has containers: refuse unless cascade is explicit; cascade is `CLUSTER_ADMIN`-only. First binary MUST implement refuse-unless-cascade for delete. `NAMESPACE_ADMIN` MUST NOT cascade-delete.
- Rename namespace: id, quotas, bindings, and namespace Raft group stay; only the unique name index moves. In-flight sessions stay bound by **id** until disconnect. New connects MUST use the new name. Target name in use → `NamespaceExists`. `NAMESPACE_ADMIN` MUST NOT rename namespaces.
- `starter_namespace acme` is create-if-absent **by name**. After `acme`→`contoso`, a later start with that starter MAY create a new empty `acme`.
- Namespace Raft majority lost: cluster store still lists the namespace, its quotas, access policies, and role bindings; schema/definition writes for that namespace fail (`06`). Quotas MUST NOT vanish because that namespace's voters are down.
- Custom role with empty permission set (slice 7): can authenticate if bound, can do nothing else.
- Billing consumers: they read metrics (`08`); this feature does not invoice.
- Tenants are **hostile** to each other; the **operator runs all nodes**. Threat model is crash-stop, not Byzantine (`12`/`16`).

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: A tenant MUST be a namespace containing schemas and data (union of container schemas, `03`).
- **FR-002**: Quotas MUST be expressible per namespace and per data type in units: bytes stored, object/row counts, connections, and optionally operation counts. Byte and row/object usage MUST be **logical** (one copy); replica factor MUST NOT multiply those counters. Hard quota enforcement is a **slice 7** complete-product requirement, not the first binary.
- **FR-003**: Exceeding a quota MUST reject the offending request with an error naming the quota, the limit, and current usage. Enforcement is a **best-effort hard reject**: in-flight requests MAY briefly exceed; later consuming requests MUST reject; requests MUST NOT hang. Quota accounting MUST NOT serialize tenant writes through the cluster primary. CPU hard isolation MUST NOT be claimed. Physical replica disk MUST NOT be used as a tenant quota unit.
- **FR-004**: Access policies MUST be attachable per tenant and per data type. Custom roles and per-data-type policies are **slice 7**.
- **FR-005**: Roles MUST include `admin`, `replication`, and `custom` with specified permissions from the `14` vocabulary. First binary MUST store and honor `admin` and `replication`; `custom` waits for slice 7.
- **FR-006**: The namespace registry, quotas, access policies, and role bindings MUST be stored in cluster-level controller storage (`06`). Namespace Raft MUST hold schemas and container definitions only. Creating a namespace MUST record it in the cluster store; it MUST NOT require a namespace-Raft read to list tenants or their caps.
- **FR-007**: Non-admin principals MUST be bound to exactly one namespace; `admin` MAY act across namespaces; `replication` authenticates nodes.
- **FR-014**: Only a principal with `CLUSTER_ADMIN` (`14`) MAY create, rename, or delete namespaces (including cascade delete) and MAY set, raise, or lower quotas. `NAMESPACE_ADMIN` MAY read quota limits and usage and MUST NOT mutate namespaces (including rename) or quotas. `CONFIGURE` on a container MUST NOT grant quota or namespace-registry writes.
- **FR-015**: A namespace MUST have an immutable id and a unique, **renameable** name. Rename MUST NOT move data, change the namespace Raft group, or rewrite role bindings (those use ids). After rename, the old name MUST NOT accept new session binds. Principal **login names** are renameable in `14`; this feature's bindings MUST key principals by id.
- **FR-008**: Each namespace MAY isolate metrics and logs from the global admin streams (`08` implements export). This isolation MUST NOT be required before `08`.
- **FR-009**: Metrics MUST be usable for quota enforcement (usage figures) and for billing consumers (`08`).
- **FR-010**: A container MAY declare encryption at rest with algorithm, key reference, and scope; this feature attaches the declaration; `14` owns unwrap. The first binary MUST accept and persist this declaration.
- **FR-011**: Key material MUST NEVER appear in namespace descriptions, configs, or logs produced by this feature.
- **FR-012**: UIs (`09`) MUST be subject to the same policies; this feature does not give UIs extra rights.
- **FR-013**: The first shippable binary (`16` slices 1–5) MUST support create, list, rename, and delete of namespaces by `CLUSTER_ADMIN`, a documented starter namespace, cluster-level `admin` and `replication` roles, and per-container encryption declarations. It MUST NOT require hard quotas, custom/per-type access policies, or private tenant metrics/logs. Namespace create/delete/rename by non-`CLUSTER_ADMIN` MUST be refused in that binary.

### Key Entities

- **Namespace**: Tenant boundary recorded in the **cluster store**. Attributes: immutable id, unique renameable name, quotas, policies, optional private telemetry settings. Schemas and container definitions are **not** attributes of this record; they live in namespace Raft (`06`).
- **Quota**: Limit + unit + optional datatype selector + current usage. Stored on the namespace record in the cluster store. For bytes and row/object counts, usage is logical size (one copy). Usage MAY lag in-flight work.
- **Access Policy**: Principal/role × datatype × allowed operations. Stored in the cluster store.
- **Role Binding**: Principal → role → namespace scope (or cluster for admin/replication). Stored in the cluster store.
- **Encryption Declaration**: Per container: algorithm, key reference, scope (consumed from `03`/`14`). Attached to the container definition in namespace Raft; this feature owns the tenancy rule that the declaration exists and never includes key material.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: First binary: a `CLUSTER_ADMIN` creates a namespace, lists it, renames it, and deletes an empty extra namespace in under 15 minutes following starter docs (starter namespace present).
- **SC-002**: Slice 7: a `CLUSTER_ADMIN` sets byte and connection quotas and a tenant writes until reject in under 15 minutes following starter docs.
- **SC-003**: Slice 7: 100% of consuming requests that arrive when usage is already at or above the cap are rejected with the quota named; 0 hangs; 100% of sibling namespaces remain writable. Concurrent in-flight work MAY exceed; 0 of those requests wait on the cluster primary solely for quota. First binary: 100% of writes with quotas unset are not quota-rejected. 100% of RF-increase cases in the suite leave logical byte/row usage unchanged (not multiplied by RF).
- **SC-004**: 100% of cross-namespace attempts by a namespace-bound principal are refused.
- **SC-005**: Slice 7: 100% of READ-only custom roles in the suite cannot write.
- **SC-006**: After `08`: tenant-only metrics endpoints contain 0 series for other namespaces in 100% of samples.
- **SC-007**: 100% of encrypted-container descriptions show algorithm and reference and 0 show key material (first binary).
- **SC-008**: After restart, 100% of first-binary roles in the suite are still present from cluster-level storage; after slice 7, 100% of quotas in the suite are still present. After a namespace Raft majority loss, 100% of that namespace's registry/quota/policy/binding records in the suite are still readable from the cluster store.
- **SC-009**: 100% of cluster-admin namespace lists in the suite succeed without contacting namespace Raft.
- **SC-010**: 100% of `NAMESPACE_ADMIN` attempts in the suite to create/delete/rename a namespace or set/raise/lower a quota are refused.
- **SC-011**: First binary: 100% of namespace renames in the suite keep id and containers; 100% of new binds to the old name fail; 100% of `NAMESPACE_ADMIN` rename attempts are refused. Principal login rename is proven in `14`.

## Assumptions

- Authentication exchanges, permission verb semantics, and **principal login-name rename** are `14`; this feature stores bindings and quotas and evaluates "is this principal allowed / over quota". Bindings use principal id, not login name.
- Tenants are hostile; the operator runs every node (crash-stop, not Byzantine). CPU hard isolation is a product non-goal; fairness is best-effort.
- Type system owns schema per container (`03`); namespace "has schema" means the union of those.
- Global vs per-namespace scrape/push is implemented by `08` using the boundary defined here; private export is not a first-binary or slice-7 gate.
- Default namespace: there is no implicit tenant; operators create namespaces explicitly (including a documented starter namespace in examples).
- Slice numbering follows `16`; first binary is slices 1–5. Namespace Raft already exists in that binary (`06`) and is the schema/definition store, not the tenancy-control store.
- Kubernetes-style "namespaced" is an API scope (quotas name a namespace). It is not a separate consensus group for those records.
- Operator-facing physical disk across replicas is `08`, not a `07` quota unit.
- "Never exceed" linearizable quota through the cluster primary is a non-goal; it would contradict the leaderless data path (`06`).

## Out of Scope

- Metric catalog and log sink wiring (`08`).
- Raft layout (`06`) except that the namespace registry, quotas, access policies, and role bindings live in cluster-level controller storage, and schemas/container definitions live in namespace Raft.
- AuthN, KMS, audit log, TLS (`14`).
- Protocol handshake (`02`) except consuming namespace binding.
- Hard quotas, custom/per-type access policies, and private tenant metrics/logs as **first-binary** requirements (they remain complete-product requirements of this feature or of `08`).
- Linearizable "never exceed" quota accounting via the cluster primary (would pin tenant writes to cluster Raft).
