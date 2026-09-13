# Contract: Node Configuration Grammar

**Feature**: `001-runtime-cli-api` | Implemented in `crates/config` | Fixtures: [`fixtures/`](fixtures/)

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
PATH        := IDENT-like token containing '/' or ':'    # e.g. /etc/x.pem, file:/etc/x.pem, vault:kv/x
comment     := '#' [^\n]*
```

Whitespace and comments are insignificant. Keywords are case-sensitive. Unknown top-level or nested directive → `unknown_directive` (error, not warning) unless a registered feature claims the block name.

## Directive reference

| Path | Arity | Type | Default | Reload | Notes |
|------|-------|------|---------|--------|-------|
| `node { name X; }` | 1 | IDENT/STRING | hostname | restart | |
| `runtime { threads N; }` | 1 | NUMBER ≥ 1 | available cores | restart | `threads auto;` is also accepted and equals omitting |
| `runtime { drain_timeout D; }` | 1 | DURATION 1s–24h | `30s` | **live** | |
| `log { level L; }` | 1 | `error|warn|info|debug|trace` | `info` | live | |
| `log { format F; }` | 1 | `text|json` | `text` | restart | |
| `admin { token_file P; }` | 1 | PATH | — | live (re-read) | Required if any admin handler enabled |
| `disable admin;` | 1 | literal | — | restart | Explicitly disables the `admin` handler |
| `disable admin-http;` | 1 | literal | — | restart | Explicitly disables the `admin-http` handler |
| `entrypoint [NAME] { … }` | 0–1 | IDENT | `<handler>@<addr>:<port>` | restart | Repeatable |
| `entrypoint.address A;` | 1 | IPv4/IPv6 | `127.0.0.1` | restart | |
| `entrypoint.port N;` | 1 | 1–65535 | — | restart | Mandatory |
| `entrypoint.handler H;` | 1 | IDENT in handler inventory | — | restart | Mandatory, exactly once |
| `entrypoint.tls { certificate P; key P; }` | 1 each | PATH | — | restart | Both mandatory inside `tls`; `client_ca` reserved |
| `buffers { NAME SIZE; }` | 1 per line | SIZE within buffer range | per-buffer default | **live** | NAME must be a registered buffer |

Directives owned by other features (reserved words, `unknown_directive` in this feature): `labels`, `storage`, `memory`, `cluster`, `namespace`, `metrics`, `ingest`, `replication`.

## Semantic rules (validate phase)

| Code | Rule | Message template |
|------|------|------------------|
| `admin_handler_undeclared` | For each of `admin`, `admin-http`: at least one entrypoint with that handler **or** a `disable <handler>;` | `handler '{h}' must be explicitly enabled (an entrypoint with 'handler {h};') or disabled ('disable {h};')` |
| `admin_handler_conflict` | Not both enabled and disabled | `handler '{h}' is both disabled and declared on entrypoint '{ep}'` |
| `admin_token_required` | `admin.token_file` present when any admin handler enabled | `admin { token_file } is required when '{h}' is enabled` |
| `admin_token_unreadable` | token file exists, readable, non-empty | `cannot read admin token file '{p}': {io}` |
| `entrypoint_missing_port` | | `entrypoint '{ep}' has no 'port'` |
| `entrypoint_missing_handler` | | `entrypoint '{ep}' has no 'handler'` |
| `entrypoint_multiple_handlers` | | `entrypoint '{ep}' declares more than one handler` |
| `entrypoint_unknown_handler` | | `entrypoint '{ep}': unknown handler '{h}'; known handlers: {list}` |
| `entrypoint_duplicate_address` | `(address,port)` unique | `entrypoint '{a}' and '{b}' both listen on {addr}:{port}` |
| `entrypoint_duplicate_name` | | `entrypoint name '{n}' used twice` |
| `threads_out_of_range` | ≥ 1 | `runtime.threads must be >= 1, got {v}` |
| `drain_timeout_out_of_range` | 1s–24h | |
| `buffer_unknown` | | `unknown buffer '{n}'; known buffers: {list}` |
| `buffer_out_of_range` | | `buffer '{n}' capacity {v} outside accepted range {min}..={max}` |
| `cert_ref_scheme_unsupported` | `vault:` etc. | `certificate reference scheme '{s}' is not supported in this version` |
| `cert_inline_forbidden` | value starts with `-----BEGIN` | `certificate material must be referenced by path, not inlined` |
| `cert_unreadable` / `key_unreadable` | | `cannot read '{p}': {io}` |
| `cert_invalid` | PEM parses | |
| `cert_not_yet_valid` / `cert_expired` | leaf validity contains now | `certificate '{p}' expired at {not_after}` |
| `unknown_directive` | | `unknown directive '{d}' at {file}:{line}:{col}` |
| `wrong_arity` | | `'{d}' expects {n} argument(s), got {m}` |
| `bad_literal` | | `'{d}': cannot parse '{v}' as {type}` |
| `syntax` | | `expected ';' or '{' after '{tok}'` |

Bind-time errors (reported before `ready`, same struct): `bind_address_in_use{ep, addr}`, `bind_address_unavailable{ep, addr}`.

Warnings (never fail): `buffers_exceed_memory{sum, available}`; `admin_handler_disabled{h}` logged at startup.

## Launch overrides

`spacestoraged --config <file> [--set <dotted.path>=<value>]...` — each `--set` is applied after parsing as if it were the last directive for that path (e.g. `--set runtime.threads=8`, `--set buffers.net.recv=128m`). Overrides are reported in the effective configuration with `source: "override"`. Entrypoints cannot be added via `--set`.

## Example (starter, also `fixtures/node.conf`)

```nginx
node {
  name db-1;
}

runtime {
  threads auto;          # one worker per available core
  drain_timeout 30s;
}

log {
  level info;
  format text;
}

admin {
  token_file /etc/spacestorage/admin.token;
}

entrypoint admin {
  address 127.0.0.1;
  port 7700;
  handler admin;
}

entrypoint admin-http {
  address 0.0.0.0;
  port 7701;
  handler admin-http;
  tls {
    certificate /etc/spacestorage/tls/admin.crt;
    key         /etc/spacestorage/tls/admin.key;
  }
}

entrypoint {
  port 9042;
  handler cassandra;     # registered by feature 02; validation fails on this build until 02 lands
}

buffers {
  net.recv      128m;
  net.send      128m;
  request.queue  32m;
}
```

Note for contract tests: `fixtures/node.conf` is validated with a handler inventory that includes a test-only stub `cassandra` handler so SC-011 holds before feature `02` exists; the production inventory in this feature contains only `admin` and `admin-http`.
