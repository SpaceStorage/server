# Contract: `spacestorage` CLI

**Feature**: `001-runtime-cli-api` | Binary crate `crates/spacestorage` | Ships in the same package as `spacestoraged` (FR-031)

## Global options

| Flag | Env | Default | Meaning |
|------|-----|---------|---------|
| `--endpoint <host:port>` | `SPACESTORAGE_ENDPOINT` | `127.0.0.1:7701` | Admin entrypoint to talk to |
| `--via <admin\|admin-http>` | `SPACESTORAGE_VIA` | `admin-http` | Which admin handler the endpoint speaks |
| `--token-file <path>` | `SPACESTORAGE_TOKEN_FILE` | `/etc/spacestorage/admin.token` | Bearer token source (never passed on the command line) |
| `--tls` | `SPACESTORAGE_TLS` | off | Use TLS to the endpoint |
| `--ca <path>` | `SPACESTORAGE_CA` | system/webpki roots | Extra trust anchor for the node's certificate |
| `--timeout <secs>` | `SPACESTORAGE_TIMEOUT` | `5` | Connect + request timeout |
| `--output <human\|json>` | `SPACESTORAGE_OUTPUT` | `human` | Output mode (FR-035) |

There is no `--insecure`; certificate verification failures are fatal (FR-030).

## Commands

| Command | Needs node | Output (human) | Output (json) |
|---------|-----------|----------------|---------------|
| `validate <config> [--set k=v]...` | no | `OK` + effective configuration summary, or numbered list of problems `file:line:col setting: message` | `{ "ok": bool, "errors": [ConfigError], "effective": EffectiveConfig? , "warnings": [] }` |
| `status` | yes | node name, state, uptime, threads total/busy, entrypoint table, buffer table | `status` result |
| `config` | yes | grouped settings with `[live]`/`[restart]` class and `pending-restart →` markers | `config` result |
| `threads` | yes | `total busy source` | `threads` result |
| `buffers` | yes | table: name, capacity, used, %, limit hits, policy | `buffers` result |
| `reload` | yes | lists `changed`, `applied live`, `pending restart`; problems if any | `ReloadReport` |
| `stop [--wait]` | yes | `draining (timeout 30s)`; with `--wait`: `stopped` or `drain timed out` | `stop` result |
| `handlers` | yes | inventory table (name, kind, owner) — convenience over `config.handlers` | `{ "handlers": [...] }` |

`validate` uses `crates/config` directly and never opens a socket; it evaluates `threads auto` against the *current* machine and prints the derived count (Story 3, scenario 3).

## Exit codes (FR-036)

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Usage error (bad flags/arguments) |
| 2 | Validation failed (`validate`, or `reload` rejected with `config_invalid`) |
| 3 | Connection failed (unreachable, timeout, TLS verification, unauthorized handshake) — message names endpoint and handler |
| 4 | Node-reported error (`invalid_state`, `internal`, `unknown_op`) |
| 5 | Reload applied with restart pending (`pending_restart` non-empty) |

## Human output conventions

- Tables are plain ASCII, aligned columns, no colour unless stdout is a TTY.
- Sizes are printed as `128 MiB (134217728)`; durations as `30s`.
- Errors go to stderr; data to stdout. In `--output json` mode stderr still carries a one-line human message on failure.

## Examples

```bash
spacestorage validate /etc/spacestorage/node.conf
spacestorage --endpoint 10.0.0.5:7701 --tls --ca /etc/ssl/corp.pem status
spacestorage --via admin --endpoint 127.0.0.1:7700 buffers --output json | jq '.buffers[] | select(.usage_ratio > 0.8)'
spacestorage reload; echo "exit=$?"     # 0 = all live, 5 = restart pending, 2 = invalid
spacestorage stop --wait
```
