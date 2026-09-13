# Quickstart: validate, start, inspect, reload, stop a SpaceStorage node

**Feature**: `001-runtime-cli-api` | Proves SC-001, SC-002, SC-004, SC-005, SC-007, SC-008, SC-009, SC-010, SC-011

Contracts referenced: [config grammar](contracts/config-grammar.md), [CLI](contracts/cli.md), [admin HTTP](contracts/admin-http.md), [admin TCP](contracts/admin-tcp-protocol.md).

## Prerequisites

- Rust 1.87 toolchain (`rustup show` — `rust-toolchain.toml` pins it).
- Linux or macOS. Ports 7700/7701 free on loopback.
- `jq` (optional, for JSON examples).

## 1. Build both binaries

```bash
cargo build --release
ls target/release/spacestoraged target/release/spacestorage     # server + CLI from one build (FR-031)
```

## 2. Prepare token and config

```bash
sudo mkdir -p /etc/spacestorage && head -c 32 /dev/urandom | base64 | sudo tee /etc/spacestorage/admin.token >/dev/null
sudo chmod 600 /etc/spacestorage/admin.token
sudo cp specs/001-runtime-cli-api/contracts/fixtures/minimal.conf /etc/spacestorage/node.conf
```

`minimal.conf` enables `admin-http` on `127.0.0.1:7701` and explicitly disables `admin`.

## 3. Validate offline (SC-004, SC-011, Story 3 scenario 2–3)

```bash
spacestorage validate /etc/spacestorage/node.conf
# expected: "OK" and an effective-configuration summary including "runtime.threads = <N> (available_cores)"

spacestorage validate specs/001-runtime-cli-api/contracts/fixtures/invalid/three-problems.conf; echo "exit=$?"
# expected: three numbered problems (threads_out_of_range, admin_handler_undeclared, unknown_directive); exit=2
```

## 4. Start the node (SC-001, SC-002, SC-005)

```bash
time spacestoraged --config /etc/spacestorage/node.conf &
# expected within 10 s (typically < 1 s): log lines
#   node db-1 state=starting
#   handler 'admin' disabled by administrator choice
#   entrypoint admin-http@127.0.0.1:7701 handler=admin-http transport=plaintext bound
#   node db-1 state=ready threads=<N> source=available_cores

curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:7701/v1/health/ready      # 200
nc -z 127.0.0.1 7700; echo "admin port closed: $?"                                    # non-zero: disabled handler never listens
```

Check thread derivation matches the machine (SC-002):

```bash
nproc   # or: sysctl -n hw.logicalcpu on macOS
spacestorage threads --output json | jq '.total, .source'
```

## 5. Inspect over the CLI and over HTTP (SC-008 parity)

```bash
spacestorage status
spacestorage config | grep -E 'live|restart'
spacestorage buffers
curl -s -H "Authorization: Bearer $(cat /etc/spacestorage/admin.token)" http://127.0.0.1:7701/v1/buffers | jq '.buffers[] | {name, capacity_bytes, used_bytes, usage_ratio}'
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:7701/v1/status                 # 401 without token (FR-026)
```

Parity check: the `buffers` JSON from the CLI (`--output json`) and from `curl` agree on `capacity_bytes`, `limit_hits_total`, `policy`.

## 6. Live reload of buffers vs restart-required change (SC-009, Story 3 scenario 8–9)

```bash
# live-only change
sudo sed -i.bak 's/^disable admin;/disable admin;\nbuffers { net.recv 256m; }/' /etc/spacestorage/node.conf
spacestorage reload; echo "exit=$?"
# expected: "applied live: buffers.net.recv"; exit=0; connections untouched

# restart-required change
printf 'runtime { threads 2; }\n' | sudo tee -a /etc/spacestorage/node.conf >/dev/null
spacestorage reload; echo "exit=$?"
# expected: "pending restart: runtime.threads"; exit=5
spacestorage config | grep 'runtime.threads'          # shows running value and "pending-restart -> 2"

# invalid change is rejected as a whole
printf 'buffers { net.recv 1k; }\n' | sudo tee -a /etc/spacestorage/node.conf >/dev/null
spacestorage reload; echo "exit=$?"                   # lists buffer_out_of_range; exit=2; nothing changed
```

## 7. Enable the TCP admin handler and TLS (SC-006)

Replace `disable admin;` with an entrypoint and add TLS to `admin-http` (self-signed cert for the test):

```bash
openssl req -x509 -newkey rsa:2048 -nodes -subj '/CN=localhost' -days 1 -keyout /tmp/admin.key -out /tmp/admin.crt 2>/dev/null
cat > /etc/spacestorage/node.conf <<'EOF'
admin { token_file /etc/spacestorage/admin.token; }
entrypoint admin      { port 7700; handler admin; }
entrypoint admin-http { port 7701; handler admin-http; tls { certificate /tmp/admin.crt; key /tmp/admin.key; } }
EOF
spacestorage stop --wait && spacestoraged --config /etc/spacestorage/node.conf &
sleep 1
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:7701/v1/health/live          # plaintext refused: curl error 52/56, not 200
spacestorage --tls --ca /tmp/admin.crt status                                           # OK over TLS
spacestorage --via admin --endpoint 127.0.0.1:7700 status                               # same answer over the TCP admin handler
```

Startup with an expired or missing certificate must fail before any port opens (edit `certificate` to a missing path and start again; expect `cert_unreadable` and no listener).

## 8. Graceful stop (SC-010, Story 1 scenarios 5–6)

```bash
# hold a connection open, then stop
( spacestorage --via admin --endpoint 127.0.0.1:7700 stop --wait ) &
spacestorage status            # may report state=draining briefly
wait; echo "server exited"
# expected: node logs state=draining, waits for in-flight work up to drain_timeout, exits 0 within timeout + 1 s
```

Second `SIGTERM` during draining exits immediately; `SIGTERM` before `ready` aborts startup without opening ports (test in `crates/node/tests/drain.rs`).

## 9. Automated equivalents

```bash
cargo test -p spacestorage-config          # grammar + fixtures (SC-004)
cargo test -p spacestorage-node            # startup, parity, tls, reload, drain, buffers
cargo test -p spacestorage                 # CLI exit codes and output modes
```

Every success criterion in the spec maps to a test file listed in [plan.md → Project Structure](plan.md#source-code-repository-root).
