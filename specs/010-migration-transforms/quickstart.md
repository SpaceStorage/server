# Quickstart: Migration, transforms, backup jobs

**Feature**: `010-migration-transforms` | **Gate**: SC-006 (< 20 min small evacuate) | Slice 10 (`016`)

Prerequisites: three-node loopback cluster from `004`/`016` harness; slice 10 compiled (`jobs.enabled on`); `MIGRATE` or `CLUSTER_ADMIN`; types `K/V Store`, `Document Store`, `Relational Table` (`003`). Admin token as `001`. Commands use [cli.md](contracts/cli.md); HTTP as [admin-http.md](contracts/admin-http.md). Job shape: [data-model.md](../data-model.md).

First-binary profile: these commands MUST fail `MigrateSlice10Required`.

## 1. Evacuate a KV container (live)

Create `acme.kv` on node A, write a small dataset. Then:

```text
spacestorage migrate --from acme.kv --to-node node-b --strategy live
spacestorage job status <id>
```

Expect: `status=completed`; reads match; node A holds no residual replica unless RF still places one (`04`). Progress shows bytes/objects while `running`. Target: under 20 minutes on this dataset (SC-006).

Anti-affinity that node-B would violate → `ConstraintUnsatisfiable`, no copy.

## 2. Copy then move between namespaces

```text
spacestorage migrate --from acme.t --to-namespace beta --policy copy
```

Expect: `beta.t` exists, `acme.t` remains. Repeat with existing `beta.t` → `NameExists`.

```text
spacestorage migrate --from acme.t --to-namespace beta --name t2 --policy move
```

Expect: `beta.t2` holds data; `acme.t` dropped (name reusable).

Quota too small at start → `QuotaExceeded` before copy. Growth past quota before cutover → job `failed`, source intact, dest not tenant-visible.

## 3. Catalog transform (understandable)

Document store `acme.profiles` with catalog mapping to `relational_table`:

```text
spacestorage transform --from acme.profiles --rewrite type_model --to-type relational_table --swap
```

Expect: SQL read of `profiles` sees mapped columns; live writes during the job appear after swap; previous container dropped (`retain_source` default false).

In-place incompatible schema via `003` still refused and points here.

## 4. Mapping query (complex)

Schema-free `acme.events` without catalog default. Use [transform-mapping-query.json](contracts/fixtures/transform-mapping-query.json):

```text
spacestorage transform --from acme.events --rewrite type_model --to-type relational_table --mapping-file transform-mapping-query.json --no-swap
```

Expect: `events_sql` rows match named columns. Omit `--mapping-file` → `MappingQueryRequired`.

## 5. Backup / restore jobs

```text
spacestorage backup --namespace acme --pitr
spacestorage restore --snapshot <id>
```

Expect: `13` snapshot id on the job; drop + restore-in-place returns logical data. Encrypted container without keys → fail naming the reference. No second snapshot format on disk.

## 6. Cancel / resume

Kill a node mid-live migrate; restart; `job status` still `running` or `paused`; `job resume` continues at `last_applied_source_seq` without duplicating keys. `job cancel` leaves source; dest incomplete and not swapped.
