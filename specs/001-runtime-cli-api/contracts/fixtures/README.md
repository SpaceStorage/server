# Configuration fixtures

Used by `crates/config` contract tests and by `crates/spacestorage/tests/cli.rs`.

- `node.conf` — starter example from the documentation (SC-011). Must validate with zero errors when the handler inventory contains `admin`, `admin-http`, and a test stub `cassandra`; certificate/token paths are substituted by the test harness with temp files.
- `minimal.conf` — smallest valid configuration.
- `invalid/*.conf` — one file per SC-004 rejection case. Each file starts with one or more `# expect: <setting> <error_code>` header lines. The test asserts that `validate()` returns **exactly** the expected `(setting, code)` pairs (order-insensitive) and nothing else; `-` as setting means "any/none" (syntax errors have no setting). `three-problems.conf` proves that all problems are reported in one run (Story 3, scenario 2).

Bind-time cases (`port in use`, `address not owned`) are not fixtures; they are exercised in `crates/node/tests/startup.rs` by pre-binding a socket.
