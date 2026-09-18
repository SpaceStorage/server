# Data Model: MVP Cut, Sequencing, and Product Non-Goals

**Feature**: `016-mvp-and-nongoals` | **Date**: 2026-09-18 | **Source**: [spec.md](spec.md) Key Entities, [research.md](research.md)

Nothing here is user data. Entities are (1) in-repo planning records and (2) compile-time / startup checks inside `spacestorage-release-profile`. Types are Rust-ish for precision.

## 1. Slice

Numbered implementation milestone. Specify order stays `01`–`16`; slices are the **build** order.

```text
enum SliceId {
    Runtime = 1,                 // 001
    TypesDurability = 2,         // 003, 013, 014 master-key subset
    PostgreSqlSubset = 3,        // 002 postgresql + first-binary dialect
    RedisSubset = 4,             // 002 redis + 015 MUST list on K/V Store
    MembershipInternodeQuorum = 5, // 011, 012, 004 replication/quorum
    RemainingHandlers = 6,       // 002 remaining + 015 MUST subset
    ControlPlaneTenancyAuthz = 7, // 006, 007, 014 full vocabulary
    QueryBeyondCrud = 8,         // 005 joins/agg/MapReduce/subscribe/2PC
    ObservabilityCatalog = 9,    // 008 live catalog
    MigrationBackup = 10,        // 010, 013 snapshot/PITR
    UisIngest = 11,              // 009
}
```

| Field | Rule |
|-------|------|
| `id` | 1..=11, unique, stable forever |
| `intent_files` | Non-empty list of `01`–`15` files this slice **implements** (not specifies) |
| `first_binary` | `id <= 5` |
| `skip_policy` | A milestone MAY omit `id > implemented_max` only as `DeferredSlice`, never by deleting intent |

Validation: `slice_unknown`, `slice_gap` (implemented set must be a prefix `1..=k`; no holes).

## 2. First Shippable Binary

Definition of done for slices 1–5. Encoded as `ReleaseProfile::FirstBinary`.

| Obligation | Source | Check |
|------------|--------|-------|
| Starter 1-node and 3-node, ladder `[az]` | FR-006 | fixtures + topology view |
| Types KV, Relational Table, Document Store | Story 1, FR-010 | KV/table CRUD; Document Store admin-create + canonical-blob CRUD |
| PostgreSQL first-binary dialect | FR-005 | [dialect-first-binary.md](contracts/dialect-first-binary.md) |
| Redis MUST list on KV; extra verbs error | FR-005, SC-006 | same |
| `quorum_domain` explicit; product write TWO / read ONE; durable WAL | FR-002, FR-009, `012`/`013` | 3-node kill-one + restart |
| One-node starter `write_quorum ONE` | FR-009 | fixture + one-node DML |
| UUID + join secret; bootstrap/join | `011` | 3-node join |
| `internode` + `replication` always listening; default loopback | FR-007 | listeners bound |
| Every entrypoint `tls` or `plaintext;` | FR-007 | omitted → startup error |
| Admin CLI/HTTP, drain | `001` | status + drain |
| Global `/metrics` for implemented paths | Story 1 | scrape |
| Master-key file | FR-007 | path required, mode ≤ 0600 |
| `multi_active=on` create refused | FR-006 | named error |
| Cassandra/ES/CH/S3/WebDAV **absent** | Story 1 sc. 6 | `entrypoint_unknown_handler` |

MUST NOT: call this binary a “v1” of the seven-protocol matrix (FR-002).

## 3. Complete Product

`ReleaseProfile::CompleteProduct` = constitution + `01`–`15`. Cargo feature `complete-product` enables remaining handlers. Slices 6–11 close this profile. Feature `016` does not specify their internals.

## 4. ReleaseProfile

```text
enum ReleaseProfile { FirstBinary, CompleteProduct }

struct HandlerBuildSet {
    required: &'static [&'static str],  // must register
    forbidden: &'static [&'static str], // must NOT register in this profile
}

struct TypeRequirement {
    required_creatable_l3: &'static [&'static str],
}
```

| Profile | Required handlers | Forbidden (must not be in the build, or if linked must not be default-on) |
|---------|-------------------|--------------------------------------------------------------------------|
| FirstBinary | `admin`, `admin-http`, `internode`, `replication`, `postgresql`, `redis` | `cassandra`, `elasticsearch`, `clickhouse`, `clickhouse-http`, `s3`, `webdav` |
| CompleteProduct | FirstBinary + the forbidden list above (all `015` handlers) | none |

`syslog` remains reserved (`001`/`009`) and is **not** required in either profile until slice 11.

Default workspace features = FirstBinary. `node` at startup: `HandlerRegistry::contains(name)` false → `entrypoint_unknown_handler{handler, known}` (collect-all, `001`).

## 5. DialectProfile

Selected per client handler from the release profile.

### PostgreSQL

| Verb / feature | FirstBinary | CompleteProduct (`015`) |
|----------------|-------------|-------------------------|
| Wire 3.0, SCRAM, simple + extended/prepared | MUST | MUST |
| INSERT/SELECT/UPDATE/DELETE, CREATE/DROP table | MUST | MUST |
| Auto-commit | MUST | MUST |
| BEGIN/COMMIT/ROLLBACK | **not-supported (`0A000`)** | MUST |
| COPY | **not-supported (`0A000`)** | MUST |
| ALTER mapped to additive schema | MAY (additive `003`) | MUST |
| PL/pgSQL, LISTEN/NOTIFY, FDW, extensions | not-supported | not-supported (non-goal of emulation) |

Error shape: PostgreSQL `ERRCODE_FEATURE_NOT_SUPPORTED` (`0A000`), never empty success and never empty-transaction notices for `COMMIT`/`ROLLBACK`.

### Redis

| Verb | FirstBinary | CompleteProduct |
|------|-------------|-----------------|
| AUTH, PING, GET, SET, DEL, EXISTS, SCAN | MUST on `K/V Store` | MUST (and type-specific ops as `015`) |
| SELECT | no-op inside bound namespace | same |
| TTL mapped to container TTL | MUST on KV | MUST |
| Type-specific verbs off KV (`HGET`, `JSON.GET`, …) | **error** (unknown command or not-supported); never success/no-op | documented type-specific ops |
| Other types (incl. Document Store) | canonical blob only | native mapping or canonical blob (`002`) |
| Cluster slots, modules, Lua, Streams-as-product | not-supported | not-supported |

## 6. ProductNonGoal

Not later. Not SpaceStorage. Stable ids for SC-005.

```text
enum ProductNonGoal {
    SecondQueryEnginePerProtocol,
    DropInReplacementOfEmulatedSystems,
    KafkaAsStoredLogProduct,
    HumanPickedMultiMasterConflict,
    ByzantineNodes,
    PerTenantCpuHardIsolation,
    NativeClientProtocol,
    SqlSerializable,
    MultiActiveOnInFirstBinary, // create refused; flag remains off
}
```

| Id | Where it is refused / Out of Scope |
|----|-------------------------------------|
| Second query engine | `005`, `002` — one `QueryEngine` |
| Drop-in replacement | `015` MUST NOT column |
| Kafka as stored log | `009` ingest, `008` outbound; type is Log Stream |
| Human conflict pick | `012` LWW/merge only |
| Byzantine | `012` crash-stop |
| CPU hard isolation | `015` |
| Native client protocol | constitution V MAY later; not a slice |
| SERIALIZABLE | `015` refuse |
| `multi_active=on` | `012`/`016` refuse create |

Audit: `nongoals.rs` lists spec paths that must mention each id. Missing mention → `nongoal_unspecified{id, spec}`.

## 7. DeferredSlice

```text
struct DeferredSlice {
    id: SliceId,              // 6..=11 for a 1–5 milestone
    reason: String,           // non-empty
    still_owed: bool,         // MUST be true unless ProductNonGoal
}
```

A deferred slice with `still_owed = false` is invalid unless the slice’s entire content is a `ProductNonGoal` (none of 6–11 are). Validation: `deferred_missing`, `deferred_marked_cancelled`, `implemented_not_prefix`.

## 8. MilestoneRecord

In-repo, not on the wire.

```text
struct MilestoneRecord {
    slug: String,                 // e.g. "first-binary"
    implemented: BTreeSet<SliceId>,
    deferred: Vec<DeferredSlice>,
    profile: ReleaseProfile,
    changelog_ref: String,        // path or tag
}
```

| Rule | Error |
|------|-------|
| `implemented` is `1..=k` | `slice_gap` |
| If `k == 5` then deferred ids == `{6,7,8,9,10,11}` | `deferred_missing` |
| `profile == FirstBinary` iff `k == 5` and handlers match FirstBinary | `profile_mismatch` |
| `profile == CompleteProduct` requires `implemented` == `1..=11` | `profile_incomplete` |

## 9. StarterTopology

| Fixture | Ladder | Nodes | `quorum_domain` | `write_quorum` |
|---------|--------|-------|-----------------|----------------|
| `first-binary-one-node.conf` | `[az]` | `db-1` `az=local` | `lab` (single member) | **ONE** (starter override) |
| `first-binary-three-node-{a,b,c}.conf` | `[az]` | `db-a/b/c` with `az=a/b/c` | `lab` (all three) | **TWO** (product default) |

Each file: admin + admin-http, postgresql, redis, internode, replication; `plaintext;` on loopback; `master_key_file`; `token_file`; bootstrap on `a` / join on `b` and `c`; `multi_active` not set (default off). One-node `query_defaults.write_quorum` MUST be ONE; three-node MUST be TWO.

RF default for the three-node smoke containers: 3, anti-affinity `az` (finest ladder key). Single-node: RF 1, no anti-affinity.

## 10. State: milestone vs node

This feature does not add a node state. It **constrains** existing ones:

- Node `ready` for first binary additionally requires: both cluster handlers bound, master-key readable, every entrypoint has transport, membership path from `011` (bootstrap or joined).
- Container create with `multi_active=on` never leaves `creating`; it fails.

## 11. Validation code inventory (this feature)

| Code | When |
|------|------|
| `entrypoint_unknown_handler` | Handler not in this build (`001`, reused) |
| `transport_undeclared` | No `tls` and no `plaintext;` (`014`, reused) |
| `internode_required` / `replication_required` | Profile requires always-on cluster ports |
| `master_key_required` / `master_key_unreadable` | First binary / `014` |
| `topology_ladder_required` | Missing `topology_ladder`; first binary default if omitted is `[az]` at resolve time |
| `multi_active_unsupported` | Create with on |
| `feature_not_supported` | PG COPY / BEGIN / COMMIT / ROLLBACK → `0A000` |
| Redis extra-verb error | `HGET` / `JSON.GET` / etc. — unknown command or not-supported; never success |
| `slice_gap` / `deferred_missing` / `profile_mismatch` | Ledger |
| `nongoal_unspecified` | SC-005 audit |

## Relationships

```text
MilestoneRecord 1──* SliceId (implemented prefix)
MilestoneRecord 1──* DeferredSlice
ReleaseProfile ── HandlerBuildSet
ReleaseProfile ── DialectProfile (per client handler)
ReleaseProfile ── TypeRequirement
ProductNonGoal ── referenced by specs 01–15 Out of Scope (audit)
StarterTopology ── ReleaseProfile::FirstBinary
CompleteProduct ── constitution + specs 001–015
```
