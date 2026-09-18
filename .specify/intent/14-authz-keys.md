---
speckit_command: specify
suggested_slug: authz-keys
source: gap-analysis-2026-09-14
read_after: 00-constitution.md
---

# Feature: Authentication, authorization, encryption in transit, key management, and audit

Specify how clients and nodes prove identity, what a role may do, how keys wrap data at rest, and how privileged actions are audited. Complements `07` (namespaces, quotas, role *names*) with the security *mechanisms* `07` left unnamed.

## What

### Principals and authentication

A **principal** is a cluster-wide identity (immutable **id**, unique **renameable** login name). Authentication uses each **client protocol's native mechanism** (`02`) and the admin surfaces' own mechanism (`01`):

- Password via SCRAM-SHA-256 (or the protocol's native password exchange mapped to the same principal store)
- Optional mTLS (certificate referenced, never inlined), per entrypoint
- Admin CLI/HTTP: bearer token or the same principal store

Until verified, a session MUST NOT see tenant data. A credential with **no namespace binding** MUST be refused on tenant protocols (Redis, S3, WebDAV, Elasticsearch and any protocol that has no native "select database" step). Protocols with a native selection step (PostgreSQL database name, Cassandra keyspace, ClickHouse database) MUST still authenticate a principal; the selected namespace MUST be one the principal is allowed to use.

**Credential → namespace**: every non-admin principal is bound to **exactly one** namespace (`02` clarification). Reaching another namespace requires different credentials. The `admin` role MAY act across namespaces. The `replication` role authenticates **nodes** on `internode` / `replication` entrypoints (`12`), not tenants.

LDAP/Kerberos MAY be added later; first binary is the local principal store in cluster-level controller storage (`06`/`07`). Tenants are **hostile** to each other; the **operator runs all nodes**.

### Authorization (permission vocabulary)

Roles (`07`): `admin`, `replication`, `custom` with specified permissions. Custom roles MUST be composed from this closed vocabulary (expandable later by constitution-adjacent amendment):

- `CLUSTER_ADMIN` — membership (`11`), global config, all namespaces
- `NAMESPACE_ADMIN` — schema and principals inside one namespace
- `READ`, `WRITE` — data plane on named containers or all containers in a namespace
- `CREATE`, `DROP` — containers in a namespace
- `CONFIGURE` — container options (placement, encryption reference) the role is allowed to set
- `REPLICATE` — internode/replication entrypoints only
- `MIGRATE` — migration/transform jobs (`10`)
- `AUDIT_READ` — read audit log
- `METRICS_READ` — scrape namespace or global metrics (`08`)

UIs (`09`) MUST enforce the same vocabulary; they MUST NOT have a private privilege model.

### Encryption in transit

Every entrypoint MUST declare **`tls { ... }`** (certificate material referenced, never inlined) **or `plaintext;`**. Omitted transport is a **startup error**. There is no silent plaintext fallback when TLS is declared, and no silent TLS-when-certs-exist. TLS is **not globally mandatory**; production vs laptop is an operator choice per port. Hostile multi-tenant SaaS (`16`) still assumes tenants are hostile: plaintext on a non-loopback tenant port is an operator-chosen exposure.

### Encryption at rest and key management

Encryption at rest is **opt-in per container** (`03`). Unencrypted persistent data on a stolen disk is an **operator-chosen leak**. Encrypted containers: stolen disk/snapshot/backup MUST be ciphertext. A **running** node that has unwrapped keys for data it hosts **can** read that data. `CLUSTER_ADMIN` can unwrap.

Algorithms that MUST be implemented:

- AES-256-GCM (default)
- ChaCha20-Poly1305

Key hierarchy: envelope encryption. A **cluster master key** (file in the first binary; later a KMS as another provider, same key **references**) wraps **per-namespace KEKs** stored in **cluster-level controller storage**. Nodes unwrap **data keys only for containers they host** and cache them in memory. A **data key** encrypts container payloads and WAL records / snapshots of that container (`13`).

**Rotate master key** = **rewrap KEKs**; data keys unchanged; no table rewrite. Master key MUST be **backupable**. **Restore data with a specified key** MUST be a documented procedure (`13`). Lost master key without backup ⇒ all **encrypted** containers (and those backups) unreadable. Unencrypted containers are unaffected. Threshold/split master keys are later.

Rotation of a **data key**: a new data key MAY be issued; old keys are retained until a transform (`10`) re-encrypts. Lost data key → that container unreadable; the error MUST name the missing key reference.

Who may bind a key reference to a container: `NAMESPACE_ADMIN` or `CLUSTER_ADMIN` for that namespace.

### Audit

Privileged actions MUST be appended to an **audit log** (cluster-global, stored in controller storage and/or the primary logging system `08`): authenticate success/failure, role changes, membership join/leave/replace, encryption key bind/rotate, backup/restore, migrate/transform, admin config changes. Audit entries MUST include principal, action, target, time (HLC/`12`). Tenants do not see cluster audit unless granted `AUDIT_READ`.

## Why

Three role names cannot be implemented. Encryption without algorithms and a master-key / KEK hierarchy cannot be implemented. Protocol sessions without a principal-to-namespace rule were already invented in spec `002` and belong in intent.

## Actors

- Tenant authenticating through a stock client
- Cluster admin managing principals, roles, and key references
- Replication subsystem authenticating to peers
- Node wrapping/unwrapping data keys
- Auditor reading the audit log
- Billing/quota remaining in `07`/`08` (this feature only gates `METRICS_READ`)

## Requirements

- Principal store; protocol-native auth mapped to it; SCRAM-SHA-256; optional mTLS; admin token.
- Login name unique and renameable; id immutable; bindings use id (`07`).
- One-namespace binding for non-admin principals; admin may cross namespaces; replication role for nodes.
- Closed permission vocabulary as listed; custom roles are sets of those permissions.
- TLS: every entrypoint `tls` or `plaintext;` — omitted is a startup error; not globally mandatory.
- Envelope keys: cluster master key wraps namespace KEKs in controller storage; rotate = rewrap; backup master key; restore with specified key.
- Encryption at rest opt-in; stolen disk of encrypted containers is ciphertext.
- WAL and backups of encrypted containers use the same key references (`13`).
- Audit log for privileged actions.

## Out of scope for this feature

- Namespace and quota **units** (`07` still owns quotas; this file does not redefine them)
- Type inventory and encryption *scope* field (`03`)
- Internode protocol framing (`12`)
- Cerebro/Kibana UI chrome (`09`) except it MUST call the same authz
- LDAP/SSO (future)
