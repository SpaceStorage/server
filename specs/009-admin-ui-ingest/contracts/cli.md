# Contract: CLI additions

**Feature**: `009-admin-ui-ingest` | Binary: `spacestorage` (`001`)

| Command | Authz | Notes |
|---------|-------|-------|
| `spacestorage ui` | any admin session | prints `admin-http` base URL + `/ui/cluster` and `/ui/console` paths |
| `spacestorage ingest kafka list` | see declaration list | JSON/table |
| `spacestorage ingest kafka add …` | `NAMESPACE_ADMIN`+`WRITE` or `CLUSTER_ADMIN` | flags: `--namespace --container --brokers --topic --group --format raw\|json` |
| `spacestorage ingest kafka delete ID` | same | |
| `spacestorage ingest syslog` | `CLUSTER_ADMIN` | **does not** bind a port; prints that syslog bind is node `entrypoint` config and points at the starter fixture |

Exit codes: `001` conventions. `ingest_write_only` / `ingest_syslog_cluster_only` → exit 3 (forbidden). `UiIngestSlice11Required` → exit 2 (config).
