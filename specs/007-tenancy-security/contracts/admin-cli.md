# Contract: Admin API and CLI

**Feature**: `007-tenancy-security` | Crates: `admin-proto`, `spacestorage`, `node` | Extends `001` CLI

## Admin ops (JSON, both handlers)

| Op | Result |
|----|--------|
| `NamespaceList` | `[NamespaceView]` from cluster store |
| `NamespaceCreate { name }` | view; `NotClusterAdmin` / `NamespaceExists` |
| `NamespaceRename { name, new_name }` | view; `NamespaceExists` / `NamespaceNotFound` |
| `NamespaceDelete { name, cascade }` | ok or `CascadeRequired` |
| `NamespaceDescribe { name }` | view + usage (usage may lag) |
| `QuotaReplace { name, quotas }` | slice 7; else `Slice7Required` |
| `RoleList` / `RoleBindingList { namespace? }` | builtin roles first binary |

Parity over `admin` and `admin-http` (`001` FR-020).

## CLI

| Command | Notes |
|---------|--------|
| `spacestorage namespaces` | table: name, id, quota summary |
| `spacestorage namespace create <name>` | CLUSTER_ADMIN |
| `spacestorage namespace rename <name> <new>` | CLUSTER_ADMIN; first binary |
| `spacestorage namespace delete <name> [--cascade]` | |
| `spacestorage namespace describe <name>` | |
| `spacestorage quotas set <name> --bytes …` | slice 7 |
| `spacestorage roles` | |

`--output json` supported. Exit codes: `001` plus 3 = retryable `Minority`/`NotLeader`; 4 = `NotClusterAdmin` / `QuotaExceeded` / `CascadeRequired`.
