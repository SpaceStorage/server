# Contract: First shippable binary

**Feature**: `016-mvp-and-nongoals` | Spec: FR-002, FR-005, FR-006, FR-007, FR-009, FR-010 | Tests: [conformance-profile.md](conformance-profile.md)

This is the definition of done for slices 1–5. It is **not** a “v1” of the seven-protocol matrix.

## MUST include

1. **Topologies** — starter one-node and three-node configs ([fixtures](fixtures/)) with cluster `topology_ladder az`. Single node: one `az` value; `query_defaults { write_quorum ONE; }` (FR-009). Three nodes: distinct `az` values `a`/`b`/`c` so RF=3 default anti-affinity (`004`) is satisfiable; `write_quorum TWO`. One `quorum_domain` (`lab`) contains every member.
2. **Types** — `K/V Store` and `Relational Table` creatable, describable, CRUD, droppable. `Document Store` admin-created; read/write as canonical blob via PostgreSQL and/or Redis (FR-010). Persistent mode for the smoke path.
3. **PostgreSQL** — see [dialect-first-binary.md](dialect-first-binary.md). No `COPY`. `BEGIN`/`COMMIT`/`ROLLBACK` all `0A000`.
4. **Redis** — `015` MUST list on `K/V Store` only; other types canonical blob. Type-specific verbs off that list error (unknown command or not-supported), never silent success (SC-006).
5. **Replication** — leaderless in the source `quorum_domain`; **product** write default `TWO` counts durable WAL acks in that domain; read default `ONE` (`012`/`013`). TWO is not `min(2, live replicas)`.
6. **Identity** — cluster UUID + join secret at bootstrap; stable node identity; join = secret + admit or one-time token (`011`).
7. **Cluster ports** — `internode` and `replication` always listening; default bind loopback; cluster address required to join remotes.
8. **Transport** — every entrypoint `tls { … }` or `plaintext;`. Omitted → startup error.
9. **Admin** — CLI + HTTP, drain (`001`).
10. **Metrics** — global `/metrics` for implemented paths (`008` families that exist).
11. **Keys** — `master_key_file`; no required external KMS.
12. **Catalog** — `multi_active` default off; create with `on` → `multi_active_unsupported`.

## MUST NOT be required (still owed unless a [non-goal](non-goals.md))

- Cassandra, Elasticsearch, ClickHouse native+HTTP, S3, WebDAV handlers (slice 6). An entrypoint naming them in this build → `entrypoint_unknown_handler`.
- Cerebro-like / Kibana-like UIs; Kafka/syslog ingest (slice 11).
- MapReduce / shuffle across regions; query beyond CRUD (slice 8).
- L0 creatable-as-day-to-day workflow (catalog MAY still describe them).
- Planetary / multi-continent **production** examples (`planet` remains a legal ladder key).
- External KMS; mixed-version rolling upgrade (MAY land with `015` once format versions exist).
- `BEGIN` / `COMMIT` / `ROLLBACK` / `COPY` on first-binary PostgreSQL (`0A000`).
- Working Redis type-specific verbs off the K/V MUST list (they MUST error).

## Build

Default `cargo build -p spacestoraged` = [release-profile](release-profile.md) `FirstBinary`. Do not publish this artifact under a name that implies seven protocols.
