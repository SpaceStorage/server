# Research: MVP Cut, Sequencing, and Product Non-Goals

**Feature**: `016-mvp-and-nongoals` | **Date**: 2026-09-18

Each item resolves a Technical Context unknown or a technology choice. Format: Decision / Rationale / Alternatives considered. Inputs: [spec.md](spec.md), constitution 1.3.0, intent `16`, sibling specs `001`–`015` (especially `002` handler registry, `004` topology ladder, `012` quorum domain, `015` compatibility ceiling).

## R1. What this feature implements (vs what it gates)

- **Decision**: `016` ships a **release profile**, **slice ledger**, **starter configs**, and **conformance suite**. It does not reimplement protocols, types, WAL, membership, or quorum. Those behaviors stay in `001`–`015`. A 1–5 milestone is done when the first-binary conformance profile passes **and** the milestone record lists slices 6–11 as deferred.
- **Rationale**: FR-003: this file sequences `01`–`15`; it does not replace them. Folding first-binary dialect rules into `002`/`015` without a profile would make the complete-product ceiling look like the ship list (the failure mode Story 1 exists to prevent).
- **Alternatives considered**: “Just write a README” (rejected: SC-002/SC-004 need machine-checkable gates); a second git repository / product fork (rejected: Principle III, doubles maintenance); encoding slices only as GitHub milestones (rejected: the repo must carry the contract).

## R2. First binary = Cargo default features, not a second binary

- **Decision**: Workspace default features enable `handler-postgresql` and `handler-redis` (plus admin, `internode`, `replication`). Cassandra, Elasticsearch, ClickHouse (both handlers), S3, and WebDAV crates are **optional** features, pulled in by `complete-product`. One binary name: `spacestoraged`. An entrypoint naming a handler not in this build fails `entrypoint_unknown_handler` at validate/start (`001` / Story 1 scenario 6).
- **Rationale**: Constitution V: shipping all seven handlers is not the first binary. Spec: handlers “are not required to be present”. Compile-time omission makes “not present” true in the artifact, not only at runtime.
- **Alternatives considered**: Always compile all handlers, return not-supported (rejected: looks like the complete product, operators will open ports); stub handlers that accept and no-op (rejected: `15` forbids silent empty success); a `spacestoraged-mvp` binary (rejected: marketing “v1”, two packages, Principle III blur).

## R3. Specify order vs implement order

- **Decision**: Numeric intent order `01`…`16` remains specify/review order. **Implementation** follows slices 1–11 in [contracts/slices.md](contracts/slices.md). A milestone that implements 1–5 MUST NOT delete later intent files. Skip = `DeferredSlice` in the milestone record.
- **Rationale**: FR-001 and Story 2. Specifying 11–16 before implementing 05–10 was already required by `.specify/intent/README.md`.
- **Alternatives considered**: Re-order specify numbers to match slices (rejected: existing branches and cross-links); implement in specify order including UIs first (rejected: Story 1 / grill).

## R4. Topology ladder default `[az]`

- **Decision**: First-binary cluster config declares `cluster { topology_ladder az; }`. Single-node starter: `labels { az local; }` (one value is enough). Three-node starter: `az a`, `az b`, `az c` so default anti-affinity (finest ladder key = `az`, `004`) can place RF=3. One explicit `quorum_domain` (`lab`) contains all three members. `planet` is allowed in the grammar; production planetary examples are not required.
- **Rationale**: Clarification Q2; constitution VII; `004` refuse-unsatisfiable anti-affinity. Three loopback processes with three `az` values keep RF=3 + default anti-affinity honest. Write `TWO` still only needs two durable source-domain acks.
- **Alternatives considered**: One shared `az` on all three nodes (rejected: RF=3 with default `az` anti-affinity would refuse); ladder `[rack, az, region]` for the first binary (rejected: clarification); inferring `quorum_domain` from `az` (rejected: `012` — domain is explicit, labels are not voting sets).

## R5. Dialect profile vs `015` ceiling

- **Decision**: `015` remains the complete-product MUST/MUST NOT matrix. First-binary PostgreSQL is a **narrower dialect profile**: wire 3.0, SCRAM, simple + extended/prepared, `INSERT`/`SELECT`/`UPDATE`/`DELETE`, `CREATE`/`DROP` table, auto-commit; `COPY`, `BEGIN`, `COMMIT`, and `ROLLBACK` all return PostgreSQL `0A000` (no empty-transaction notices). First-binary Redis is the `015` Redis MUST list **on `K/V Store` only**; other types = canonical blob; a type-specific Redis command off that list (`HGET`, `JSON.GET`, …) MUST return a Redis error (unknown command or not-supported) and MUST NOT succeed or no-op. Slice 6 must speak the **full** `015` MUST subset, not a private unpublished list (Story 2 scenario 2).
- **Rationale**: FR-005; Session 2026-09-18 Q3–Q4; `015` FR-004/FR-008 already point at `16`. One profile object (`DialectProfile::FirstBinary` vs `CompleteProduct`) is selected at handler construct from `release-profile`, so adding `BEGIN` later is a profile switch, not a rewrite. Silent success on extra Redis verbs would look like a complete Redis.
- **Alternatives considered**: Changing `015` MUST to match the first binary (rejected: that would cancel constitution V); compile-time `cfg` scattered in the PG crate without a profile type (rejected: slice 6 would miss verbs); `COMMIT`/`ROLLBACK` as ordinary empty-transaction notices (rejected: Session 2026-09-18 Q3); leaving extra Redis verbs untested if they happen to work (rejected: Session 2026-09-18 Q4).

## R6. Required types vs full `003` catalog

- **Decision**: First binary **must** create/use `K/V Store`, `Relational Table`, `Document Store`. `Document Store` is **admin-created**; first-binary read/write is the `002` **canonical blob** over PostgreSQL and/or Redis. Native document verbs (Redis JSON, Elasticsearch) are not in this binary. The `003` catalog may still **list** other types (drivers verify mappings at startup for **registered** types). Creating L3 types whose only native protocol is absent is served via PG/Redis canonical blob when the type is in the first-binary set; otherwise create MAY be refused. L0 creatable-as-day-to-day workflow is later (FR-008); catalog + conformance MAY still unit-test L0 creatable flags.
- **Rationale**: Story 1 type list; FR-010; Session 2026-09-18 Q2. A create-only Document Store would fail SC-002. Native document APIs would pull Elasticsearch or Redis JSON into slices 1–5.
- **Alternatives considered**: Hide all other types from the catalog (rejected: `003` identical-catalog-per-release; slice 6/8 would then “add types”); require all ten L3 models in the first binary (rejected: Story 1); Document Store create/describe only (rejected: Q2); Redis JSON as a first-binary MUST (rejected: Q2/Q4).

## R7. Query engine: CRUD now, `005` beyond-CRUD later

- **Decision**: First-binary DML/DDL uses the `002` `QueryEngine` seam with enough of `005` to run auto-commit CRUD (insert/select/update/delete, create/drop). Joins, aggregation, MapReduce, subscribe, distributed transactions, `BEGIN` are **slice 8** (and `BEGIN` is also excluded by the first-binary dialect). Conformance does not require EXPLAIN; complete product still does (`015`).
- **Rationale**: Slice 8 is “query engine beyond CRUD”. First-binary PG smoke is CRUD. Implementing full `005` before three-node quorum would invert the slice order.
- **Alternatives considered**: Block PG until `005` is complete (rejected: never ships); implement `BEGIN` early because `015` lists it (rejected: FR-005).

## R8. Control plane: interim store until slice 7

- **Decision**: Slices 1–5 persist membership, namespace/schema descriptors, and shared-datatype metadata on the `004`/`011` `ClusterStore` seam (append-only log over `internode`). Slice 7 replaces the seam with Raft (`006`) without changing first-binary client tests. First binary does **not** require namespace quotas (`007`) or the full `014` permission matrix beyond SCRAM/AUTH, admin token, replication role on cluster ports, and master-key wrap.
- **Rationale**: Constitution XII is owed; `016` records it deferred. Three-node quorum is a **data-path** property (`004`/`012`/`013`), not Raft.
- **Alternatives considered**: Require Raft before any three-node test (rejected: slice order 5 then 7); skip membership and run three independent single-node processes (rejected: Story 1 kill-one + write TWO).

## R9. Always-on `internode` and `replication`

- **Decision**: First-binary configs **must** declare both handlers. Default bind `127.0.0.1`. Join of remotes requires a cluster address (not loopback-only) on those entrypoints (`012`). `disable internode` / omitting the entrypoint is a startup error for this profile (`internode_required`, `replication_required`), overriding `004`’s earlier optional internodes for the first-binary and complete-product profiles. Transport on every entrypoint is `tls { … }` or `plaintext;` (`014`).
- **Rationale**: FR-007; Story 1 scenario 1. `004` planned internodes as enable/disable like admin; `012` made both always-on. The release profile is the place that **enforces** the later rule on starter configs.
- **Alternatives considered**: Keep internodes optional until a second node joins (rejected: spec — listen even on one node); reuse a client protocol port (rejected: constitution V / `012`).

## R10. Durable write `TWO` / read `ONE`

- **Decision**: Product / three-node default remains write `TWO`, read `ONE`. Counted write acks for persistent/hybrid containers are **durable WAL** in the source `quorum_domain` only (`012`/`013`). The **one-node starter sets `write_quorum ONE`** so single-replica DML can complete (FR-009). TWO is **not** `min(2, live durable replicas)`: a one-node process left on product default TWO MUST fail writes that need two acks. First-binary three-node conformance: kill one, write at default TWO, restart, read back. Memory-mode is not the smoke storage mode.
- **Rationale**: Constitution VIII; FR-009; Session 2026-09-18 Q1. Starter override trains operators that TWO is still the product default (it is visible in the three-node file).
- **Alternatives considered**: First-binary **product** default write `ONE` (rejected: would train operators on an unsafe default); TWO = `min(2, live replicas)` (rejected: Q1 option B); one-node path start/admin only with no DML (rejected: Q1 option C); counting in-memory acks (rejected: `013`).

## R11. Master-key file, no KMS

- **Decision**: First binary requires `cluster { master_key_file <path>; }` (mode ≤ 0600) wrapping namespace KEKs in controller storage (`014`). External KMS is later-not-first (FR-008). Lost key without backup ⇒ encrypted data unrestorable (`014`); starter examples use encryption **opt-in off** so the 60-minute path does not depend on key backup.
- **Rationale**: FR-007; `014` clarification Q2.
- **Alternatives considered**: Skip keys until slice 7 (rejected: spec lists master-key file in the first binary); require KMS (rejected: FR-008).

## R12. `multi_active=on` refused

- **Decision**: Catalog flag exists, default `off`, ordered/log types forced off (`012`). Create with `on` returns a named refuse (`multi_active_unsupported`) in **both** first-binary and complete-product until a non-HLC dual-active merge exists. First-binary conformance asserts the refuse. This is a **product non-goal for the first binary** and remains refused until that merge — not a slice-6 surprise enable.
- **Rationale**: Story 1 scenario 5; non-goal list; `012` FR-011.
- **Alternatives considered**: Omit the flag from the catalog (rejected: `012` wants the flag visible); silently ignore `on` (rejected: silent success).

## R13. Observability in the first binary

- **Decision**: Every ready node exposes global `/metrics` (Prometheus text) with labels that exist for **implemented** paths: at least `001` runtime/buffer/node_state, protocol traffic for postgresql/redis, durability WAL counters used by the TWO test, replication ack counters. Full `08` family list is slice 9. Billing **formula** is a non-goal of this repo; series that `08` marks for billing still emit when those paths exist.
- **Rationale**: Story 1; FR-008 vs non-goals; constitution X without forcing unimplemented HNSW/LSM series.
- **Alternatives considered**: No `/metrics` until slice 9 (rejected: Story 1); emit the entire `08` catalog as zeros (rejected: false completeness).

## R14. Product non-goals vs later-not-first

- **Decision**: Two lists in [contracts/non-goals.md](contracts/non-goals.md). Non-goals MUST appear as Out of Scope (or named refuse) in the relevant specs and MUST NOT be Jira “later”. Later-not-first items stay in `01`–`15` and in deferred slices. Kafka is ingest (`09`) and outbound logging (`08`), not a stored log product (Log Stream is the type).
- **Rationale**: FR-004, FR-008, Story 3. Grill closed SERIALIZABLE, second engines, drop-in replacement, human-in-the-loop conflict, Byzantine, CPU hard isolation, native client protocol.
- **Alternatives considered**: A single “won’t do yet” bucket (rejected: that is how non-goals re-enter as backlog).

## R15. Conformance harness

- **Decision**: Extend `crates/conformance` (from `002`/`004`) with `#[cfg(feature = "first-binary")]` tests that boot 1 then 3 in-process nodes on ephemeral ports, using stock client crates `tokio-postgres` and `redis`. Complete-product handler smokes stay behind `feature = "complete-product"` and are **not** run in the default CI job for a 1–5 milestone. G4 asserts `COPY`/`BEGIN`/`COMMIT`/`ROLLBACK` → `0A000`. G5b asserts `HGET`/`JSON.GET` Redis errors. G11 asserts admin-create Document Store + canonical-blob CRUD. A cassandra entrypoint fixture asserts validate failure. One-node effective config must show `write_quorum ONE`.
- **Rationale**: SC-001–SC-003, SC-006; Story 1 Independent Test. Reuse, don’t fork, the harness.
- **Alternatives considered**: Only docker-compose manual tests (rejected: not CI-gateable); requiring psql/redis-cli binaries in CI (optional in quickstart; crate clients in automated tests).

## R16. Milestone record format

- **Decision**: `docs/milestones/<nnn>-<slug>.md` plus a machine-readable `docs/milestones/<nnn>-<slug>.yaml` listing `implemented: [1..k]` and `deferred: [k+1..11]`. `release-profile::ledger` fails CI if a record claims `implemented` includes 1–5 and `deferred` does not equal `{6,7,8,9,10,11}` (or a documented later set when more slices land). Changelog of the repo release MUST link the record (SC-004).
- **Rationale**: Story 2 scenario 1 — skipping must be recorded, not absent.
- **Alternatives considered**: Git tags only (rejected: no deferred list); editing intent files to comment out slices (rejected: FR-003).

## R17. One-node write quorum (Session 2026-09-18 Q1)

Covered by R10. Starter override, not a change to constitution VIII.

## R18. Document Store first-binary path (Session 2026-09-18 Q2)

Covered by R6. Admin-create + canonical blob; no Elasticsearch in this binary.

## R19. PostgreSQL transaction trio (Session 2026-09-18 Q3)

Covered by R5. `BEGIN`/`COMMIT`/`ROLLBACK` share one `0A000` rule.

## R20. Redis extra verbs (Session 2026-09-18 Q4)

Covered by R5. Error, never silent success. Conformance G5b.
