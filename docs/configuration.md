# SpaceStorage node configuration

This document describes the nginx-style configuration grammar for `spacestoraged`
and offline validation via `spacestorage validate`. It is derived from
`specs/001-runtime-cli-api/contracts/config-grammar.md`.

A starter example lives at [`docs/examples/node.conf`](examples/node.conf).

## Lexical grammar

```text
file        := item* EOF
item        := directive | block
directive   := IDENT arg* ';'
block       := IDENT arg* '{' item* '}'
arg         := IDENT | NUMBER | SIZE | DURATION | STRING | PATH
IDENT       := [A-Za-z_][A-Za-z0-9_\-.]*
NUMBER      := [0-9]+
SIZE        := NUMBER ('k'|'m'|'g'|'K'|'M'|'G')?        # binary units: k=KiB, m=MiB, g=GiB
DURATION    := NUMBER ('ms'|'s'|'m'|'h')
STRING      := '"' ( [^"\\] | '\\' . )* '"'
PATH        := IDENT-like token containing '/' or ':'
comment     := '#' [^\n]*
```

Whitespace and comments are insignificant. Keywords are case-sensitive. An unknown
top-level or nested directive is a hard error (`unknown_directive`) unless a
registered feature claims the block name.

## Directive reference

| Path | Arity | Type | Default | Reload | Notes |
|------|-------|------|---------|--------|-------|
| `node { name X; }` | 1 | IDENT/STRING | hostname | restart | |
| `runtime { threads N; }` | 1 | NUMBER ≥ 1 | available cores | restart | `threads auto;` equals omitting |
| `runtime { drain_timeout D; }` | 1 | DURATION 1s–24h | `30s` | **live** | |
| `log { level L; }` | 1 | `error\|warn\|info\|debug\|trace` | `info` | live | |
| `log { format F; }` | 1 | `text\|json` | `text` | restart | |
| `admin { token_file P; }` | 1 | PATH | — | live (re-read) | Required if any admin handler enabled |
| `disable admin;` | 1 | literal | — | restart | Explicitly disables the `admin` handler |
| `disable admin-http;` | 1 | literal | — | restart | Explicitly disables the `admin-http` handler |
| `entrypoint [NAME] { … }` | 0–1 | IDENT | `<handler>@<addr>:<port>` | restart | Repeatable |
| `entrypoint.address A;` | 1 | IPv4/IPv6 | `127.0.0.1` | restart | |
| `entrypoint.port N;` | 1 | 1–65535 | — | restart | Mandatory |
| `entrypoint.handler H;` | 1 | IDENT in handler inventory | — | restart | Mandatory, exactly once |
| `entrypoint.tls { certificate P; key P; }` | 1 each | PATH | — | restart | Both mandatory inside `tls` |
| `entrypoint.plaintext;` | 0 | — | — | restart | Explicit plaintext; every enabled entrypoint must declare `tls` or `plaintext` |
| `buffers { NAME SIZE; }` | 1 per line | SIZE within buffer range | per-buffer default | **live** | NAME must be a registered buffer |

Reload class (`live` vs `restart_required`) is also reported in the effective
configuration (`GET /v1/config` / admin `config` op) under `settings[]`.

Directives reserved for other features (rejected as `unknown_directive` in this
feature): `labels`, `storage`, `memory`, `cluster`, `namespace`, `metrics`,
`ingest`, `replication`.

## Built-in buffers

| Name | Default | Range | Overflow |
|------|---------|-------|----------|
| `net.recv` | 64 MiB | 1 MiB–64 GiB | wait |
| `net.send` | 64 MiB | 1 MiB–64 GiB | wait |
| `request.queue` | 16 MiB | 1 MiB–16 GiB | reject |

## Semantic validation (selected codes)

| Code | Rule |
|------|------|
| `admin_handler_undeclared` | Each of `admin` / `admin-http` must have an entrypoint **or** `disable <handler>;` |
| `admin_handler_conflict` | Not both enabled and disabled |
| `admin_token_required` / `admin_token_unreadable` | Token file required and readable when any admin handler is enabled |
| `entrypoint_duplicate_address` | `(address,port)` unique |
| `threads_out_of_range` / `drain_timeout_out_of_range` | Range checks |
| `buffer_unknown` / `buffer_out_of_range` | Buffer inventory and capacity ranges |
| `cert_*` | File-referenced PEM only; leaf must be currently valid at startup |

Warnings (never fail startup): `buffers_exceed_memory`; disabled admin handlers are
logged as an administrator-choice notice.

## Launch overrides

```text
spacestoraged --config <file> [--set <dotted.path>=<value>]...
```

Examples: `--set runtime.threads=8`, `--set buffers.net.recv=128m`. Entrypoints
cannot be added via `--set`.

## Example

See [`docs/examples/node.conf`](examples/node.conf) (copied from the feature
fixture used by contract tests).
