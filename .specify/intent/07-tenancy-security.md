---
speckit_command: specify
suggested_slug: tenancy-security
source: server/start
read_after: 00-constitution.md
---

# Feature: Namespaces, quotas, access policies, encryption, and roles

Specify multi-tenancy and security for SpaceStorage.

## What

The database is **multi-tenant**. Every tenant is called a **namespace**. Each namespace has **schema and data** inside it, like a relational database.

Each tenant MAY have:

- different **quotas** for different types of data
- different **access policies** for different types of data
- accumulated **metrics and logs** of its own

Quota **units** MUST include bytes stored, object/row counts, connections, and optionally operation counts, per namespace and per data type (`15`). At the limit the request is **rejected** with a named error. CPU hard isolation is a product non-goal (`15`, `16`); fairness is best-effort.

Tenants are **hostile** to each other; the **operator runs all nodes**. The threat model is crash-stop, not Byzantine (`12`, `16`).

Metrics are also used for **billing and quota management**. Authentication, permission vocabulary, master key / KEKs, TLS, and audit are specified in `14`; this feature owns namespaces, quotas, access-policy attachment, and the three role **names** (`admin`, `replication`, `custom`).

Each namespace MAY configure an API to expose metrics relevant to that namespace only, to its own monitoring system, separately from the primary `/metrics` endpoint (personal API endpoint per namespace, or push to OpenTelemetry). Logging MAY be sent to the tenant’s own logging system separately from the primary logging system. Treat this as **per-namespace statistics and logging** plus **global** statistics and logging for the SpaceStorage administrator. (Exact series and log sinks are in `08`; this feature owns the tenancy boundary and the right to isolate them.)

### Encryption

For security, each node MAY **encrypt data before storing it** in drives or memory when it is needed and specified for the **data container**. Encryption mechanisms include different algorithms and different keys (see L0 in `03`).

### Roles

A role system MUST manage the database cluster. Roles:

- **admin**
- **replication role**
- **custom role** with specified permissions

Roles MUST be stored in **controller storage on the cluster level**.

## Why

Many customers share one cluster with isolated schema/data, quotas, keys, and optional private telemetry, while cluster admins keep global view and role authority.

## Actors

- Cluster admin with the admin role
- Replication subsystem authenticating with the replication role
- Tenant admin with a custom role scoped to a namespace
- Billing system consuming namespace metrics
- Node encrypting a container to disk or memory

## Requirements

- Namespace = tenant; contains schemas and data.
- Quotas and access policies per tenant and per data type.
- Optional per-container encryption at rest (disk or memory).
- Cluster-level role store: admin, replication, custom-with-permissions.
- Tenant-optional private metrics/log export distinct from global admin streams.

## Out of scope for this feature

- Full metrics catalog (`08`)
- Control-plane Raft layout (`06`) except that roles live in cluster-level controller storage
- AuthN mechanisms, permission verbs, KMS, audit log (`14`)
- Encryption algorithms and envelope keys (`14`); this feature still states that encryption is per container when specified
