# Research: Namespaces, Quotas, Access Policies, Encryption Attachment, and Roles

**Feature**: `007-tenancy-security` | **Date**: 2026-09-16

Each item resolves a Technical Context unknown or a technology choice. Format: Decision / Rationale / Alternatives considered. Inputs: [spec.md](spec.md) (clarify 2026-09-16), constitution 1.3.0, intent `07`, sibling plans `006` (cluster vs namespace logs), `003` (encryption fields), `005` (admission), `002`/`014` (binding and verbs), `008` (export), `015` (IOPS-equivalent ops), `016` (slices).

## R1. One crate interprets cluster tenancy records

- **Decision**: Add `crates/tenancy` (`spacestorage-tenancy`). It defines `NamespaceRecord`, `Role`, `RoleBinding`, `QuotaSpec`, `AccessPolicy` and serializes them as cluster-log bodies. `controlplane` routes `ClusterStore::append` for these events to the **cluster** group (not namespace). `006` R18's opaque `roles: Vec<u8>` is replaced by these typed records; the Raft crate still does not interpret permission verbs.
- **Rationale**: `006` R1 kept `openraft` out of `placement`. Same reason: tests of name/quota logic must not boot Raft.
- **Alternatives considered**: Grow `controlplane` (rejected: Raft tests vs admit tests); store records in namespace Raft (rejected: clarify Q2).

## R2. Namespace create starts namespace Raft

- **Decision**: `NamespaceCreate` is one cluster-log commit (registry row). Apply handler then asks `006` to start `GroupId::Namespace(id)` with voter set copied from cluster (existing `006` R5). List/get of tenants reads **only** cluster applied state (SC-009). Delete without cascade refuses if the namespace catalog has any container; cascade is `CLUSTER_ADMIN` and then destroys the namespace group.
- **Rationale**: Spec FR-006; `006` already creates a group per namespace.
- **Alternatives considered**: Config-only namespaces (rejected: SC-001 admin create); two-phase client (rejected: one admin op).

## R3. Builtin roles in the first binary

- **Decision**: Bootstrap (or first apply) inserts two roles: `admin` (permissions include `CLUSTER_ADMIN` as an opaque set supplied by `14` when present; until `14` ships, the name `admin` is the gate) and `replication` (`REPLICATE` / internodes identity). First-binary `001` admin token maps to `admin`. Internodes/`replication` entrypoints map to `replication`. **Custom** roles and the closed verb set wait for slice 7 + `14`. Bindings: `admin` has cluster scope; `replication` has cluster (node) scope; non-admin principals bind to exactly one namespace (`02`/`14`).
- **Rationale**: FR-005/007/013. Smoke needs a cluster admin and node identity.
- **Alternatives considered**: Delay all roles to slice 7 (rejected: clarify Q1); encode verbs in this crate (rejected: `14` owns vocabulary).

## R4. Quota caps in Raft, usage not in Raft

- **Decision**: `QuotaSpec` (limit, unit, optional type selector) is on the namespace cluster record. **Usage** is an in-memory ledger per `(namespace_id, unit, type_selector)` seeded by summing **logical** container sizes from catalog (one number per container, not per replica). Coordinators `check` then `fetch_add`; on data-path failure `fetch_sub`. Best-effort internodes `QuotaDelta` spreads deltas; periodic reconcile resets to catalog sum. **No** usage entry in the cluster log.
- **Rationale**: Clarify Q5. Caps must survive restart (cluster log). Usage that waited on Raft would pin writes.
- **Alternatives considered**: Raft-committed usage (rejected: Q5 option B); disk-bytes from `stat` (rejected: Q3 physical).

## R5. Logical size, not replica factor

- **Decision**: Byte and row/object usage = user-visible logical size of containers in the namespace (catalog). Changing RF (`04`) MUST NOT change those counters. Physical replica disk is an `08` operator series, not a quota unit.
- **Rationale**: Clarify Q3.
- **Alternatives considered**: Physical sum (rejected: RF burns tenant cap).

## R6. Optional operation counts are per-second

- **Decision**: When set, operation-count quotas are **IOPS-equivalent per second** (token bucket, refill 1s). Omitted = unlimited. Not in the first binary. Matches `015`.
- **Rationale**: Clarify deferred window; `15` already said IOPS-equivalent. Lifetime counters are not a quota.
- **Alternatives considered**: Calendar day (billing, not admit); lifetime (cannot reset without admin rewrite of usage).

## R7. Connection quota is concurrent authenticated sessions

- **Decision**: Unit `connections` counts **authenticated sessions** bound to that namespace, cluster-wide (best-effort sum). TCP handshakes that fail auth do not count. Increment on bind, decrement on close. Same overshoot rules as bytes.
- **Rationale**: Spec “connections”; `015` also has per-entrypoint limits — the **stricter** of entrypoint max and namespace quota wins (`005` pattern).
- **Alternatives considered**: Raw TCP (rejected: scanners consume tenant cap); per-node only (rejected: tenant could multiply by member count).

## R8. Who writes access policies

- **Decision**: Only `CLUSTER_ADMIN` may attach, replace, or drop **access policies** in the cluster store (same as quotas). `NAMESPACE_ADMIN` cannot. Custom role **contents** (verb sets) remain `14`; this crate stores the binding and the policy rows.
- **Rationale**: Policies are isolation controls on the cluster registry. Clarify Q4 for quotas; policies are the same class of record (clarify Q2).
- **Alternatives considered**: `NAMESPACE_ADMIN` writes policies (would let a tenant lift their own deny); `CONFIGURE` (spec FR-014 forbids CONFIGURE for registry writes).

## R9. Namespace names are renameable labels

- **Decision**: Unique cluster-wide, **case-sensitive**, 1–63 chars, `^[A-Za-z][A-Za-z0-9_]*$`. Maps 1:1 to PostgreSQL database name / Cassandra keyspace / ClickHouse database (`02`). Duplicate → `NamespaceExists`. Starter example name: `acme`. **Id is immutable**; **name is renameable** in the first binary via `NamespaceRename` (`CLUSTER_ADMIN` only). Rename updates the unique name index only: Raft `group_id`, quotas, bindings, and container data stay. After commit, new binds use the new name; in-flight sessions stay on namespace **id** until disconnect. `ALTER DATABASE … RENAME` maps to this op. `starter_namespace` is create-if-absent **by name**. Principal login rename is `14`; this crate keys `RoleBinding.principal_id`.
- **Rationale**: Product MUST: names change; ids must not. First-binary CRUD already hits the cluster log.
- **Alternatives considered**: UUID-only names (breaks `psql -d acme`); case-insensitive (surprises PG); rename out of scope (rejected: 2026-09-16 product decision).

## R10. Encryption declaration

- **Decision**: Fields live on the **container definition** in namespace Raft (`03`): `algorithm` (`AES-256-GCM` default, `ChaCha20-Poly1305`), `key_ref` (string reference, never material), `scope` (`drives` | `drives_and_memory`). This crate’s describe path **redacts** any accidental key bytes (`KeyMaterialForbidden` if a write tried to inline). Memory-mode + `drives` → refuse (`03`). First binary MUST persist and show the declaration. Unwrap is `14`.
- **Rationale**: FR-010/011; clarify Q1.
- **Alternatives considered**: Duplicate declaration on the namespace record (rejected: per-container); inline keys (rejected: FR-011).

## R11. Private telemetry flags wait for `08`

- **Decision**: Optional `private_metrics` / `private_logs` booleans on `NamespaceRecord`. First binary: omitted or `off` stored; `on` at validate is `ObservabilityRequired` (same pattern as `006` exclusive-data / slice 7). Export endpoints and sinks are `08`. Billing consumers read `08`; this crate does not invoice.
- **Rationale**: Clarify Q1; FR-008.
- **Alternatives considered**: Implement scrape in `07` (rejected: metric catalog is `08`).

## R12. Unset quota means unlimited

- **Decision**: Missing quota row ⇒ no reject for that unit. Zero ⇒ reject consuming work immediately (namespace still exists). Lowering below usage is allowed; subsequent consuming requests reject until usage falls.
- **Rationale**: First-binary FR-013; spec edge cases.
- **Alternatives considered**: Mandatory quotas at create (rejected: first binary; also SaaS may start unlimited).

## R13. Admission order with `005`

- **Decision**: Coordinator path: authz (`14`) → **tenancy admit** (quota + access policy) → query admission (`005` node/namespace slots). Named error `quota_exceeded { quota, limit, usage }`. Never hang. `005` `query.max_concurrent_per_namespace` vs connection/op quotas: **stricter wins**.
- **Rationale**: `005` admission.md already says `07` MAY lower the cap.
- **Alternatives considered**: Quota inside the planner (too late; connections never reach the planner).

## R14. Protocol mapping

- **Decision**: PostgreSQL `CREATE DATABASE name` / `DROP DATABASE` map to `NamespaceCreate` / `NamespaceDelete` and require `CLUSTER_ADMIN` (else protocol permission error). `psql -d acme` requires `acme` to exist in the registry. Redis AUTH binds via `14` to a registry namespace; `SELECT` stays no-op inside the bound namespace (`16`). This crate does not implement SCRAM.
- **Rationale**: `02` FR-009; spec FR-007.
- **Alternatives considered**: Admin-CLI-only create (rejected: PG-native create is expected for CLUSTER_ADMIN; CLI still required for SC-001).

## R15. Metrics

- **Decision**: This crate increments reserved `08` names: `spacestorage_namespace_usage_bytes`, `spacestorage_namespace_usage_objects`, `spacestorage_namespace_connections`, `spacestorage_quota_exceeded_total`. Labels: `namespace`, `unit`, optional `datatype`. Do not rename. Exposition remains `08`.
- **Rationale**: FR-009; constitution X.
- **Alternatives considered**: Private names (rejected: observability contract).

## R16. Cargo features vs `016`

- **Decision**: First binary compiles `tenancy` without `tenancy-quotas`. Slice 7 enables quota set/enforce, custom roles, access policies. `016` slice 7 already listed tenancy quotas and full authz; this plan does **not** pull those into slices 1–5 (unlike `006` Raft).
- **Rationale**: Clarify Q1 vs `006` FR-020 (Raft had to move; quotas did not).
- **Alternatives considered**: Full `07` in first binary (rejected: Q1 option C).

No `NEEDS CLARIFICATION` remains in Technical Context.
