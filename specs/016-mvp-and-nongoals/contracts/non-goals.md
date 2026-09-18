# Contract: Product non-goals vs later-not-first

**Feature**: `016-mvp-and-nongoals` | Spec: FR-004, FR-008 | Audit: SC-005

Two lists. Mixing them is a spec defect.

## Product non-goals (not later; not SpaceStorage)

These MUST appear as Out of Scope or a named refuse in the cited specs. They MUST NOT be implied by type or protocol inventories. They MUST NOT be “we’ll see” tickets.

| Id | Statement | Owning spec(s) | Enforcement |
|----|-----------|----------------|-------------|
| `SecondQueryEnginePerProtocol` | One `QueryEngine`; handlers lower into it | `002`, `005` | crate graph: handlers do not bundle a second planner |
| `DropInReplacementOfEmulatedSystems` | Stock **clients** on MUST verbs; unmodified **applications** that need MUST-NOT verbs are out of scope | `015` | MUST NOT column → protocol not-supported |
| `KafkaAsStoredLogProduct` | Kafka is ingest (`009`) and outbound logging (`008`); Log Stream is the L3 type | `003`, `008`, `009` | no Kafka log handler as storage |
| `HumanPickedMultiMasterConflict` | LWW by HLC or type-supported deterministic merge only | `012` | no pause-for-human API |
| `ByzantineNodes` | Crash-stop failure detection | `012` | heartbeat/timeout only |
| `PerTenantCpuHardIsolation` | Fairness is best-effort | `015`, `007` | no claim in quotas |
| `NativeClientProtocol` | Constitution MAY add later; not a slice in 1–11 | constitution V, `002` Out of Scope | no `handler spacestorage` in inventories |
| `SqlSerializable` | Isolation is `READ COMMITTED` / `SNAPSHOT` only | `015`, `005` | `SERIALIZABLE` refused |
| `MultiActiveOnInFirstBinary` | Catalog flag default off; create-on refused | `012`, this feature | `multi_active_unsupported` |

`crates/release-profile/src/nongoals.rs` maps each id to the spec paths that must mention it. `nongoal_unspecified` fails the audit test.

## Later, not first binary (still in intent)

Remain in `01`–`15`. A 1–5 milestone lists them via deferred slices, not as non-goals.

| Item | Slice / spec | Notes |
|------|----------------|-------|
| Remaining protocol handlers at `015` MUST | 6 | |
| Raft control plane, quotas, full authz vocabulary | 7 | |
| Query beyond CRUD | 8 | includes complete-product `BEGIN` |
| Full `008` catalog live | 9 | series MAY appear earlier |
| Migration/transforms, snapshot/PITR | 10 | |
| Admin UIs and Kafka/syslog ingest | 11 / `009` | UIs MAY be deferred even later; spec `009` still applies to complete product |
| Full L0 creatable-as-user-workflow | `003` | catalog + unit conformance MAY still test flags |
| Federated / union / materialized view at planetary scale | `003` L4 / `004` | |
| External KMS | `014` | master-key file is first binary |
| Mixed-version N/N+1 upgrade | `015` | MAY land once format versions exist (`013`) |
| Billing money formula | — | out of this repo; billing **metrics** remain `008` |
| GDPR / legal-hold workflows | — | residency is labels + placement; erase is delete/tombstone/`010` |
| CDC as a named product beyond Log Stream + WAL | — | |

## Explicitly out of this feature

- Contents of `01`–`15` except the first-binary subsets named here.
- Marketing version numbers beyond slice names (“v1”, “GA”).
- CI/CD of this repository (beyond the conformance **commands** in [quickstart.md](../quickstart.md)).
