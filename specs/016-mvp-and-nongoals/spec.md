# Feature Specification: MVP Cut, Sequencing, and Product Non-Goals

**Feature Branch**: `016-mvp-and-nongoals`

**Created**: 2026-09-15

**Status**: Draft

**Input**: User description: "Read .specify/intent/16-mvp-and-nongoals.md and specify this feature." — implementation slices 1–11; first shippable binary = slices 1–5; complete product still owes `01`–`15`; product non-goals (not later); explicit later-not-first-binary items.

## Clarifications

### Session 2026-09-18

- Q: On the one-node starter, how should a default write at TWO be treated when only one durable replica exists in the source quorum domain? → A: One-node starter sets `write_quorum ONE`; product/three-node default stays TWO.
- Q: For the first binary, how must `Document Store` be proven, given PostgreSQL and Redis are the only client protocols? → A: Admin-create Document Store; CRUD as canonical blob via PostgreSQL and/or Redis.
- Q: If PostgreSQL `BEGIN` is not-supported in the first binary, what should `COMMIT` and `ROLLBACK` return? → A: `BEGIN`/`COMMIT`/`ROLLBACK` all not-supported (`0A000`).
- Q: If a first-binary Redis client sends a type-specific command that is not on the K/V MUST list (for example `HGET` or `JSON.GET`), what must happen? → A: Error (unknown command or not-supported); never silent success.

### Session 2026-09-15

- Q: What is the first shippable binary? → A: Slices 1–5: runtime; types+durability on one node; PostgreSQL smoke (no COPY, no BEGIN); Redis MUST list on `K/V Store` only; membership + internode + clocks + three-node quorum. Not the seven-protocol matrix. Constitution MUSTs that this file defers are still owed, not cancelled.
- Q: Default topology ladder for that binary? → A: `[az]` (one value is enough on a single node).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - First shippable binary (Priority: P1)

An early operator runs a single-node and a three-node cluster. Types: `K/V Store`, `Relational Table`, `Document Store`. `Document Store` is created through admin; first-binary read/write is the canonical blob over PostgreSQL and/or Redis (native document verbs are not required). PostgreSQL: wire 3.0, SCRAM, simple + extended/prepared, INSERT/SELECT/UPDATE/DELETE, CREATE/DROP table, auto-commit; no COPY; `BEGIN`/`COMMIT`/`ROLLBACK` all not-supported (`0A000`). Redis: AUTH, PING, GET, SET, DEL, EXISTS, SCAN, SELECT as no-op, TTL mapped — on `K/V Store` only; other types = canonical blob; type-specific verbs off that MUST list return a Redis error (unknown command or not-supported), never success or no-op. Leaderless replication in an explicit `quorum_domain`; product default write ack==2 / read ack==1 inside the source domain, durable WAL acks. The **one-node starter overrides write quorum to ONE** so single-replica DML can complete; the **three-node starter and product default stay TWO**. Stable node identity, UUID + join secret, bootstrap/join. `internode` and `replication` always listening; default bind loopback. Every entrypoint `tls` or `plaintext;`. Admin CLI/HTTP, drain. Global `/metrics` for implemented paths. Cluster master-key file. `multi_active=on` create refused.

**Why this priority**: This is the definition of done for the first binary; without it implementers will treat `15` as the ship list.

**Independent Test**: Bring up 1-node (write quorum ONE) then 3-node (write quorum TWO) from starter examples; PG and Redis smokes; admin-create a `Document Store` and CRUD it as canonical blob via PG and/or Redis; kill one node; write at TWO; restart; confirm restore.

**Acceptance Scenarios**:

1. **Given** starter configs for one node and three nodes with ladder `[az]`, **When** they start, **Then** membership, `internode`/`replication`, and admin surfaces work, and omitted transport fails startup.
2. **Given** PostgreSQL smoke as listed, **When** COPY, BEGIN, COMMIT, or ROLLBACK is issued, **Then** not-supported `0A000` (first binary); DML/DDL auto-commit succeeds.
3. **Given** Redis MUST list on `K/V Store`, **When** those commands run, **Then** they succeed; **When** a type-specific verb off that list is sent (`HGET`, `JSON.GET`, …), **Then** Redis returns an error (unknown command or not-supported), never silent success.
4. **Given** a `Document Store` created through admin, **When** the operator reads and writes it over PostgreSQL and/or Redis, **Then** those operations use the canonical blob mapping; native document verbs are not required.
5. **Given** three nodes, **When** a write uses default write quorum, **Then** two durable source-domain acks are required (`12`/`13`).
6. **Given** the one-node starter, **When** DML runs without a per-query quorum override, **Then** it uses starter `write_quorum ONE` and succeeds on the single durable replica; product default TWO is unchanged.
7. **Given** `multi_active=on` create, **When** it is submitted, **Then** it is refused.
8. **Given** Cassandra/ES/CH/S3/WebDAV handlers, **When** this binary is built, **Then** they are **not** required to be present; an entrypoint naming them fails as unknown handler (`01`).

---

### User Story 2 - Sequencing without deleting intent (Priority: P1)

A maintainer implements slices in order. Skipping a later slice in a milestone MUST be recorded as deferred, not deleted from `01`–`15`. Slices 6–11 complete the intent surface (remaining protocol handlers at `15` MUST, Raft control plane, tenancy quotas, full authz, query beyond CRUD, observability catalog live, migration/backup, UIs/ingest).

**Why this priority**: Prevents "we'll see" deletion of constitution obligations.

**Independent Test**: Planning checklist maps each slice to intent files; confirm 6–11 still listed as owed.

**Acceptance Scenarios**:

1. **Given** a milestone that ships slices 1–5 only, **When** its changelog is read, **Then** slices 6–11 are listed deferred, not absent from intent.
2. **Given** slice 6, **When** it is done, **Then** remaining protocol handlers speak the `15` complete-product MUST subset (not a private smaller unpublished list).

---

### User Story 3 - Product non-goals stay out of specs (Priority: P1)

Specs MUST list these as Out of Scope, not "later": a second query engine per protocol; drop-in replacement for every feature of the emulated systems; Kafka as a stored log product; multi-master conflict that waits for a human; Byzantine nodes; per-tenant CPU hard isolation; a SpaceStorage-native client protocol; SQL `SERIALIZABLE`; `multi_active=on` in the first binary.

**Why this priority**: Grill closed these; they otherwise reappear as silent backlog.

**Independent Test**: Review specs `01`–`15` Out of Scope / non-goals; none of the list is a MUST.

**Acceptance Scenarios**:

1. **Given** `SERIALIZABLE` or a second query engine, **When** a spec would require it, **Then** that spec is non-conformant with this feature.
2. **Given** Kafka, **When** it appears, **Then** it is ingest (`09`) and outbound logging (`08`), not a stored log product (Log Stream is the type).

---

### Edge Cases

- One-node starter DML uses `write_quorum ONE` (config override). TWO is not reinterpreted as “min(2, live replicas)”; a one-node process left on product default TWO must fail writes that need two acks.
- First-binary `Document Store` has no Elasticsearch (or other document-native) handler. Create is admin; protocol I/O is canonical blob. Redis JSON / `HGET` / similar type-specific verbs MUST error (unknown command or not-supported), never succeed or no-op.
- First-binary PostgreSQL has no open-transaction path: `COMMIT`/`ROLLBACK` are not-supported like `BEGIN`, not empty-transaction notices.
- UIs and ingest (`09`) MAY be deferred in an implementation milestone; the `09` spec still applies to the complete product.
- Planetary / multi-continent **examples** in production are not required for the first binary; `planet` remains a legal ladder key; `quorum_domain` is already required.
- Billing money formula is out of this repo; metrics for billing remain (`08`).
- GDPR/legal-hold workflows are later; residency is labels + placement.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Implementation MUST follow the slice order in the intent (runtime → types+durability → PostgreSQL subset → Redis subset → membership/internode/three-node quorum → remaining handlers → control plane/tenancy/authz → query beyond CRUD → observability catalog → migration/backup → UIs/ingest). Specify order remains numeric intent order.
- **FR-002**: The first shippable binary MUST be slices 1–5 as listed in the intent and Story 1. It MUST NOT be described as a "v1" of the seven-protocol matrix.
- **FR-003**: Complete-product obligations in `01`–`15` remain. Skipping a slice in an implementation milestone MUST be recorded as deferred, not deleted from intent.
- **FR-004**: Product non-goals listed in the intent MUST appear as Out of Scope in the relevant specs and MUST NOT be implied by type or protocol inventories.
- **FR-005**: First-binary PostgreSQL MUST NOT require COPY. First-binary PostgreSQL MUST return not-supported (`0A000`) for `BEGIN`, `COMMIT`, and `ROLLBACK`. First-binary Redis MUST implement the K/V MUST list only as required verbs; a type-specific command off that list MUST return a Redis error (unknown command or not-supported) and MUST NOT succeed or no-op.
- **FR-006**: First-binary topology ladder default MUST be `[az]`. Catalog `multi_active` default off; create with on refused.
- **FR-009**: Product default write quorum MUST remain TWO and read quorum ONE (`12`/`13`). The one-node starter MUST set `write_quorum ONE`. The three-node starter MUST keep TWO. TWO MUST NOT mean `min(2, live replicas)`.
- **FR-007**: First binary MUST include master-key file (no required external KMS), always-on `internode`/`replication` (default loopback), and explicit `tls` or `plaintext;` on every entrypoint.
- **FR-008**: Explicit later-not-first-binary items in the intent (UIs/ingest, full L0 creatable-as-workflow, planetary production examples, external KMS, mixed-version upgrade, federated/union at planetary scale, billing formula, GDPR workflows, CDC beyond Log Stream + WAL) remain in complete-product intent unless listed as non-goals.
- **FR-010**: First binary MUST allow admin-create of `Document Store` and MUST support read/write of that container as a canonical blob through PostgreSQL and/or Redis. Native document verbs MUST NOT be required.

### Key Entities

- **Slice**: Numbered implementation milestone.
- **First Shippable Binary**: Slices 1–5 definition of done.
- **Complete Product**: Constitution + `01`–`15`.
- **Product Non-Goal**: Not later; not SpaceStorage.
- **Deferred Slice**: Still owed; recorded in the milestone.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A new operator following starter docs brings up the first-binary three-node cluster and completes PG and Redis smokes in under 60 minutes.
- **SC-002**: 100% of first-binary conformance runs exclude Cassandra/ES/CH/S3/WebDAV handlers as requirements, refuse `multi_active=on` create, and include admin-create plus canonical-blob CRUD for `Document Store` via PostgreSQL and/or Redis.
- **SC-003**: 100% of COPY, BEGIN, COMMIT, and ROLLBACK attempts on first-binary PostgreSQL return not-supported (`0A000`).
- **SC-006**: 100% of first-binary Redis commands off the K/V MUST list (e.g. `HGET`, `JSON.GET`) return an error; none succeed or no-op.
- **SC-004**: Planning artifacts for a 1–5-only milestone list slices 6–11 as deferred in 100% of reviews.
- **SC-005**: 100% of product non-goals in the intent are Out of Scope (or refused) in the corresponding specs.

## Assumptions

- This feature does not replace `01`–`15`; it sequences them.
- Numeric specify order is unchanged; implementation of a slice MAY skip later UI/ingest work.
- Starter examples for single-node and three-node live in `04` documentation deliverables; the one-node starter's `write_quorum ONE` is a lab override, not a product-default change.

## Out of Scope

- The contents of `01`–`15` themselves (except the first-binary subsets named here).
- Marketing version numbers beyond slice names.
- CI/CD of this repository.
