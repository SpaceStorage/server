# Contract: Config directives for ingest

**Feature**: `009-admin-ui-ingest` | Grammar base: [001 config-grammar.md](../../001-runtime-cli-api/contracts/config-grammar.md)

`001` already reserves the top-level word `ingest`. This feature claims it and the `entrypoint.ingest` child for `handler syslog`.

## Kafka (optional on a node; cluster object is source of truth)

Node config MAY list brokers for local override later; v1 Kafka declarations are **cluster metadata** via admin API/CLI. Starter files may include a comment-only example. If a node file contains:

```text
ingest kafka NAME {
    namespace NS;
    container C;
    type log_stream;          # default
    format raw;               # or json
    brokers "host:9092";      # repeatable
    topic T;
    group G;
    plaintext;                # or tls { certificate P; key P; }
}
```

on a **first-binary** profile → `UiIngestSlice11Required`. On slice 11, a file-local `ingest kafka` is accepted as bootstrap **only when the cluster store is empty** for that `NAME`; afterward the cluster object wins (`006`). Reload class: **live**.

## Syslog entrypoint child

| Path | Arity | Type | Default | Reload |
|------|-------|------|---------|--------|
| `entrypoint.ingest.namespace` | 1 | IDENT/STRING | — | restart |
| `entrypoint.ingest.container` | 1 | IDENT/STRING | — | restart |
| `entrypoint.ingest.type` | 1 | IDENT | `log_stream` | restart |

Required when `handler syslog`. Unknown handler `syslog` on first-binary inventory → `entrypoint_unknown_handler` or `UiIngestSlice11Required` (profile must pick one and tests lock it: **`UiIngestSlice11Required`** when the profile is first-binary and syslog is enabled).

## Buffers

| Name | Default | Range | Policy | Owner |
|------|---------|-------|--------|-------|
| `ingest.syslog.recv` | 16 MiB | 1 MiB–1 GiB | reject | `09` |
| `ingest.kafka.decode` | 32 MiB | 1 MiB–1 GiB | reject | `09` |
