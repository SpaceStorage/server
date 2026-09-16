# SpaceStorage speckit intent blocks

Source of truth: [`../../start`](../../start) (original unstructured product intent).

These files split that intent into **one Spec Kit command per file**. Do not paste `start` into a single `/speckit.specify` run. Spec Kit reads **one feature per specify invocation**; constitution is separate from feature specs.

## How to use

1. Initialize Spec Kit in `spacestorage/server` if `.specify/memory/` and skills are not present yet:

   ```bash
   specify init --here --force --non-interactive --integration cursor-agent --ignore-agent-tools
   ```

   Keep this `intent/` directory. It is input, not generated output.

2. Constitution first (non-negotiable rules):

   ```text
   /speckit.constitution Read .specify/intent/00-constitution.md and apply it as the SpaceStorage constitution.
   ```

3. Then one specify per remaining file, in numeric order. Always attach the file:

   ```text
   /speckit.specify Read .specify/intent/01-runtime-cli-api.md and specify this feature.
   ```

   Repeat for `02` … `16`.

4. After each spec: `/speckit.clarify` if needed, then `/speckit.plan`, `/speckit.tasks`, `/speckit.implement`.

   Do not start `05`–`10` specify until `11`–`16` exist (identity, internode/clocks, durability, authz/keys, compatibility, MVP). Implement against the MVP slice in `16`, not the entire surface at once.

## File map

| File | Spec Kit command | Suggested feature slug | What it covers |
|------|------------------|------------------------|----------------|
| `00-constitution.md` | `/speckit.constitution` | — | Rust/Tokio/monolith, type-driven multiparadigm, protocol-per-port, quorum defaults, observability methodology, governance |
| `01-runtime-cli-api.md` | `/speckit.specify` | `runtime-cli-api` | Process model, thread config, CLI + HTTP/TCP admin surfaces |
| `02-protocols-drivers.md` | `/speckit.specify` | `protocol-drivers` | PostgreSQL, Cassandra, Redis, Elasticsearch, ClickHouse, S3, WebDAV; drivers over abstract types |
| `03-type-system.md` | `/speckit.specify` | `type-system` | L0–L4 primitives, encodings, compression, encryption, expandable lists |
| `04-distribution-placement.md` | `/speckit.specify` | `distribution-placement` | L1 shared: topology ladder, labels, disks, memory, quorum, sync/async replication, anti-affinity |
| `05-query-execution.md` | `/speckit.specify` | `query-execution` | Parser/planner/scheduler/executor, MapReduce, transactions, timeouts, fault tolerance |
| `06-control-plane.md` | `/speckit.specify` | `control-plane` | Cluster/namespace/datatype/node primaries, Raft, restore on boot |
| `07-tenancy-security.md` | `/speckit.specify` | `tenancy-security` | Namespaces, quotas, access policies, RBAC, at-rest encryption |
| `08-observability.md` | `/speckit.specify` | `observability` | Prometheus/OTel metrics catalog, Four Golden Signals, per-namespace metrics/logs, billing |
| `09-admin-ui-ingest.md` | `/speckit.specify` | `admin-ui-ingest` | Cerebro-like and Kibana-like UIs; Kafka/syslog ingest |
| `10-migration-transforms.md` | `/speckit.specify` | `migration-transforms` | Cross-node/namespace migration; datatype/storage-model transforms |
| `11-identity-membership.md` | `/speckit.specify` | `identity-membership` | Cluster/node IDs, seeds, bootstrap, join, drain, decommission, replace |
| `12-internode-and-time.md` | `/speckit.specify` | `internode-and-time` | `internode`/`replication`, `quorum_domain`, source vs log-follower, HLC in-domain, LWW |
| `13-durability-and-recovery.md` | `/speckit.specify` | `durability-and-recovery` | WAL/fsync acks, restore, `gc_grace`/TTL/compaction, snapshot/PITR |
| `14-authz-keys.md` | `/speckit.specify` | `authz-keys` | AuthN, permission vocabulary, explicit TLS/plaintext, master key / KEKs, audit |
| `15-compatibility-and-limits.md` | `/speckit.specify` | `compatibility-and-limits` | Complete-product MUST/MUST NOT, isolation (no SERIALIZABLE), size limits, N/N+1 upgrade |
| `16-mvp-and-nongoals.md` | `/speckit.specify` | `mvp-and-nongoals` | Build slices, first shippable binary (1–5), product non-goals |

## Reading rules for Spec Kit

- Treat each file as the **full `$ARGUMENTS`** for that command.
- Preserve MUST-level requirements. Do not drop lists (types, metrics, labels).
- Constitution content stays in `00`. Specify files may repeat a constraint only when the feature cannot be understood without it.
- Metric names, protocol names, and datatype names in these files **are product requirements**, not accidental implementation detail.
- `start` remains the original dump. If a later edit conflicts, prefer the numbered intent file for that domain, then reconcile `start`.
