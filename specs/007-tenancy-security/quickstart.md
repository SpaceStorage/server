# Quickstart: namespaces, builtin roles, encryption, slice-7 quotas

**Feature**: `007-tenancy-security`

Proves SC-001, SC-004, SC-007, SC-008, SC-009, SC-010, SC-011 on the first binary. SC-002, SC-003, SC-005 need slice 7 (`tenancy-quotas`). SC-006 needs `08`. Principal login rename: [014 spec](../../014-authz-keys/spec.md) SC-007.

Prerequisites and one-node / three-node start: [016 quickstart](../../016-mvp-and-nongoals/quickstart.md). Append [namespace-block.conf](contracts/fixtures/namespace-block.conf) so starter `acme` exists.

## 0. Config validation

```bash
spacestorage validate specs/007-tenancy-security/contracts/fixtures/invalid/quota-negative.conf
# expected: exit 2, QuotaNegative

spacestorage validate specs/007-tenancy-security/contracts/fixtures/invalid/quotas-on-first-binary.conf
# expected: exit 2, Slice7Required on first-binary profile

spacestorage validate specs/007-tenancy-security/contracts/fixtures/invalid/encryption-key-inline.conf
# expected: exit 2, KeyMaterialForbidden
```

## 1. Starter namespace and list without namespace Raft (SC-001, SC-009)

Start the one-node starter.

```bash
spacestorage namespaces --output json
# expected: acme present; list succeeds even before any table exists
```

`psql -h 127.0.0.1 -p 5432 -U demo -d acme` works (registry name = database name).

## 2. Create, second namespace, delete empty (SC-001)

```bash
spacestorage namespace create otherns
spacestorage namespaces   # acme and otherns
spacestorage namespace rename otherns otherns2
# expected: id unchanged; psql -d otherns2 works; psql -d otherns fails until name reused
# NAMESPACE_ADMIN rename → NotClusterAdmin (exit 4)
spacestorage namespace delete otherns2
# expected: otherns2 gone; acme unchanged
```

Delete `acme` while it has containers without `--cascade` → `CascadeRequired` (exit 4).

## 3. Non-admin cannot create (SC-010)

With a principal bound to `acme` as `NAMESPACE_ADMIN` (or first-binary tenant user):

```bash
# namespace create stolen
# expected: NotClusterAdmin (exit 4), acme/otherns unchanged
```

Same for `namespace rename`.

## 4. Encryption declaration, no key material (SC-007)

Create a persistent container with `algorithm`, `key_ref`, `scope drives`. Describe:

```bash
spacestorage namespace describe acme --output json
# container description: algorithm + key_ref + scope; 0 key bytes
```

Memory-mode + `scope drives` → `EncryptionScopeInvalid`.

Restart the node: roles and namespace list still present (SC-008).

## 5. Cross-namespace refuse (SC-004)

Tenant bound to `acme` opens `otherns` (re-create it first) → protocol/authz refuse. `admin` may operate on both.

## 6. Slice 7 quotas (optional; SC-002, SC-003)

Build with `tenancy-quotas`. As `CLUSTER_ADMIN`:

```bash
spacestorage quotas set acme --bytes 1048576 --connections 10
# write until QuotaExceeded { quota: bytes, limit, usage }; 0 hangs
# otherns remains writable
# raise replica factor on an acme container: usage bytes unchanged
# NAMESPACE_ADMIN quotas set → NotClusterAdmin
```

Concurrent writers near the cap MAY briefly exceed; the next consuming request rejects; none wait on the cluster primary for quota.

## 7. After `08` (optional; SC-006)

Enable private metrics for `acme`. Tenant scrape contains 0 series for other namespaces. Global `/metrics` still has admin view.
