# Quickstart: Multiparadigm Type System

**Feature**: `003-type-system` | **Plan**: [plan.md](plan.md) | Validates every success criterion of [spec.md](spec.md)

A walkthrough on a fresh single node: read the catalog, create a container at each level, configure storage and protection, declare capabilities, compose, evolve a schema, restart, and inspect. Commands use the `spacestorage` CLI from `001`; every step is equally reachable over `admin-http` and, for container operations, over any `002` protocol.

## Prerequisites

- The workspace builds: `cargo build --release` (binaries `spacestoraged`, `spacestorage`).
- A config from [contracts/fixtures/node-typed.conf](contracts/fixtures/node-typed.conf), an admin token file, a users file (`002`) and a keyring from [contracts/fixtures/keyring.example](contracts/fixtures/keyring.example) at mode `0600`.

```bash
spacestorage validate --config node-typed.conf          # offline, no node needed
spacestoraged --config node-typed.conf &
spacestorage status                                     # node_state: ready
```

Expected: `ready` within a second; `storage.data_dir` now contains `catalog/` and nothing else.

## 1. Read the catalog — SC-001, SC-007

```bash
spacestorage types                       # 55 types, level and kind per row
spacestorage types --level L3            # the ten storage models
spacestorage type timeseries             # full descriptor
spacestorage codecs                      # encodings + codecs + algorithms with applicability
spacestorage capabilities                # 13 capabilities and which types support each
spacestorage catalog                     # release id + digest
```

Expected: every name from the inventory appears once at its stated level; `spacestorage type <name>` shows modes, schema kind, operations with their ordering/filtering claims, the capability row, allowed layout components, codecs, canonical payload kinds and a starter example. These calls read metadata only — `storage.block_cache` usage stays at zero.

## 2. Create one container per level with no options — SC-002, SC-003

```bash
spacestorage create tenant-a.orders     type=relational_table schema=@orders.json
spacestorage create tenant-a.cache      type=kv_store                 # no other options
spacestorage create tenant-a.docs       type=document_store
spacestorage create tenant-a.idx        type=bplus_tree               # an L0 primitive, directly (Q1)
spacestorage describe tenant-a.cache
```

Expected: each create succeeds and `describe` shows the type's **default** layout, mode and codec with a `default` marker on every option the caller did not set. Repeating this for all ten L3 models — plus one write, one read and one type-specific operation each — is the fifteen-minute path SC-002 measures; the same loop runs the starter example of every descriptor for SC-003.

Creating a storage primitive is refused:

```bash
spacestorage create tenant-a.raw type=sstable
# NotCreatable{type=sstable}: sstable is a storage primitive used by lsm_tree, kv_store, relational_table, …
```

## 3. Configure storage, encoding, compression and encryption — SC-004, SC-011

```bash
spacestorage create tenant-a.metrics \
  type=timeseries mode=hybrid hybrid.window=7d schema=@schema-metrics.json \
  encoding.ts=delta encoding.value=gorilla encoding.host=dictionary \
  compression=zstd compression.level=3 \
  encryption.algorithm=aes-256-gcm encryption.key=kv-rambler/data/ss/tenant-a encryption.scope=drives

spacestorage create tenant-a.session_cache \
  type=kv_collection mode=memory \
  encryption.algorithm=chacha20-poly1305 encryption.key=kv-rambler/data/ss/tenant-b \
  encryption.scope=drives_and_memory                       # required for memory mode (Q5)

spacestorage describe tenant-a.metrics
```

Expected: both descriptions report algorithm, **key reference** and scope — never key material. Declaring `encryption.scope=drives` on the memory-mode container is refused with `EncryptionScopeInvalid{mode=memory, scope=drives}` naming the required scope. A round trip through every mode × encoding × codec × algorithm pair is what the conformance matrix automates for SC-004.

## 4. Declare shared capabilities — SC-006

```bash
spacestorage alter tenant-a.metrics \
  capability.replication.factor=3 capability.replication.anti_affinity=az \
  capability.partitioning.scheme=time capability.partitioning.key=ts capability.partitioning.interval=1d

spacestorage alter tenant-a.idx capability.sharding.key=id
# CapabilityUnsupported{...} when the type's matrix row says so — the message lists the supported set
```

Expected: accepted declarations appear in `describe`; a container with none is described as *using placement defaults*, and those defaults are named (with the interim director: one replica, node-local). Declarations are validated before anything reaches the placement layer, so a rejected declaration leaves the container exactly as it was.

## 5. Compose — SC-009

```bash
spacestorage create tenant-a.metrics_archive type=timeseries schema=@schema-metrics.json
spacestorage create tenant-a.metrics_all type=union compose.members=metrics,metrics_archive

spacestorage describe tenant-a.metrics_all       # kind, members, depth, write rule, computed operation set
psql -h 127.0.0.1 -p 5432 -c "INSERT INTO metrics_all VALUES (now(), 'h1', 1.0)"
# refused: CompositionReadOnly{union} naming the members — nothing stored (Q3)
```

Expected: `metrics_all` is listed as a container with its type name through every protocol; a cyclic or cross-namespace definition is refused; writes to `union` and `materialized_view` are refused, while `federated`, `partitioned`, `sharded`, `replicated` and `distributed` route a write whose key resolves to exactly one member and refuse anything ambiguous.

## 6. Evolve a schema — SC-009a

```bash
spacestorage alter tenant-a.orders schema=@orders-v2.json    # adds a nullable column, widens i32 -> i64
spacestorage describe tenant-a.orders                        # schema.version: 2

spacestorage alter tenant-a.orders schema=@orders-narrow.json
# IncompatibleSchemaChange{change=narrow(total: i64 -> i32), transform_hint=…}
```

Expected: the additive change completes immediately regardless of container size, previously written rows read back with the new column as `null`, and the change is visible through every protocol at once (PostgreSQL `\d`, Cassandra `system_schema`, Elasticsearch mapping, ClickHouse `DESCRIBE TABLE`). The incompatible change is refused and stored data is untouched.

## 7. Restart — FR-023, SC-004 (second half)

```bash
spacestorage stop && spacestoraged --config node-typed.conf &
spacestorage containers tenant-a
spacestorage describe tenant-a.session_cache
```

Expected: `orders`, `metrics`, `docs`, `idx` and `metrics_all` come back with definitions, options and **data** intact; `session_cache` comes back with its definition and options, **empty**, and its description states that its content did not survive the restart and that replication is how content returns (Clarification Q2). A container whose key cannot be resolved comes back `Unavailable{KeyUnresolvable}` — present, refusing operations, nothing discarded.

## 8. Registration isolation — SC-010

With a test type, encoding, codec, algorithm and capability registered in `typeset` behind a test feature flag:

```bash
spacestorage types | wc -l                 # +1
spacestorage describe tenant-a.metrics     # byte-identical to the pre-registration output
spacestorage create tenant-a.probe type=test_type
```

Expected: every pre-existing description is unchanged byte-for-byte, the new capability shows as `Unsupported` for every existing type until a descriptor opts in, and the new items are selectable immediately.

## 9. Observability — SC-012

```bash
spacestorage buffers                        # types.memory, storage.memtable, storage.block_cache, storage.wal, catalog.metadata
spacestorage describe tenant-a.metrics      # stats: items, bytes_stored, bytes_memory, accesses, hits/misses
```

Expected: access counts, hit/miss counts where the type has that notion, and operation durations are tracked per type and namespace and are available to `08` for exposition and to `07` for quotas.

---

## Success-criteria map

| Criterion | Step | Automated by |
|---|---|---|
| SC-001 | 1 | `conformance/tests/catalog_complete.rs` |
| SC-002 | 2 | `conformance/tests/starter_examples.rs` (timed L3 loop) |
| SC-003 | 2 | `conformance/tests/starter_examples.rs` |
| SC-004 | 3, 7 | `matrix_modes_codecs.rs`, `restart_restore.rs` |
| SC-005 | 2, 3, 5 | `invalid_definitions.rs` (one case per code, plus a filesystem and catalog assertion that nothing was created) |
| SC-006 | 4 | `capability_matrix.rs` |
| SC-007 | 1 | `operations_reachable.rs` |
| SC-008 | 3 | `canonical_all_types.rs` |
| SC-009 | 5 | `compositions.rs` |
| SC-009a | 6 | `schema_evolution.rs` |
| SC-010 | 8 | `registration_isolation.rs` |
| SC-011 | 3 | `encryption_scope.rs` |
| SC-012 | 9 | `type_stats.rs` |

## Known limitations at this feature's boundary

- Replication, sharding and placement are **declared and validated** here; they are executed by `04`. The interim director reports one replica, so a memory-mode container has no way to regain content after a restart yet.
- The catalog is node-local; `spacestorage catalog --cluster` reports per-node differences but there is no cluster-wide catalog until `06`.
- Keys come from the interim `keys { keyring_file }`; `07` replaces the provider without changing any container definition.
- Changing `mode` or `layout` after creation, and any incompatible schema change, require the transform mechanism of `10`.
