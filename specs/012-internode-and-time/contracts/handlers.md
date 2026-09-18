# Contract: Always-on `internode` and `replication` handlers

**Feature**: `012-internode-and-time` | Crates: `internode`, `replication`, `node` | Spec: FR-001–FR-003 | Extends [`001` entrypoints](../../001-runtime-cli-api/spec.md)

## Required entrypoints

Every node MUST declare exactly these cluster handlers (in addition to admin declarations from `001`):

```text
entrypoint { address 127.0.0.1; port 7000; handler internode; plaintext; }
entrypoint { address 127.0.0.1; port 7001; handler replication; plaintext; }
```

| Rule | Code |
|------|------|
| Missing `internode` entrypoint | `internode_required` |
| Missing `replication` entrypoint | `replication_required` |
| `disable internode;` or `disable replication;` | `internode_cannot_disable` |
| Shared port with `admin` / `admin-http` / tenant / each other | `entrypoint_port_conflict` (`001`) |
| Omitted `tls` / `plaintext` | `transport_required` (`001`/`014`) |
| Address omitted | default `127.0.0.1` (not `0.0.0.0`) |
| Listen is loopback and `cluster { join; }` with a remote seed | `cluster_address_required` |

They MUST accept connections when the node is `ready`, including a single node with no peers (SC-001).

## Auth

Join secret (`011` `cluster.token_file`) MUST verify before any payload except the secret itself. Failure → close. First binary: secret verify = replication role. Unauthenticated peers refused.

## Advertise

`011` membership records internodes and replication listen addresses. Heartbeats use internodes. Bulk streams use replication.
