# Fixtures for feature `003-type-system`

Two kinds of fixture, both consumed by contract tests in `crates/conformance`.

## 1. Node configurations (`*.conf`)

| File | Purpose |
|---|---|
| `node-typed.conf` | Starter node with `storage`, `memory`, `types`, `keys` and the new buffers. Must pass `spacestorage validate` unchanged; it is also the source of `docs/examples/node-typed.conf`. |
| `keyring.example` | Interim `KeyAuthority` file format (feature `07` replaces it). Test material only. |
| `invalid/*.conf` | One file per new configuration validation code. The test asserts the exact code, the offending setting and the line. |

## 2. Container definitions (`containers/*.def`, `invalid/definitions/*.def`)

A `.def` file is the flat option namespace of [`container-definition.md`](../container-definition.md), one `key = value` per line, `#` comments, `namespace` and `name` giving the target. The harness feeds them to the admin `create-container` op.

| File | Demonstrates |
|---|---|
| `containers/l0-bplus-tree.def` | an L0 data structure created directly (Clarification Q1) |
| `containers/l2-vector-collection.def` | required schema, hybrid mode, ANN layout, encryption, replication |
| `containers/l3-timeseries.def` | per-field encodings, time partitioning, hybrid window |
| `containers/l4-union.def` | a read-only composition (Clarification Q3) |

`invalid/definitions/*.def` carries one case per definition validation code. Each file's first comment line is `# expect: <Code>`, and the test asserts that code, that the message names the offending element and the accepted alternatives, and that **nothing was created** — no catalog record, no directory under `storage.data_dir`, no placement declaration (SC-005).

## 3. Relationship to other features

`001`'s `fixtures/node.conf` and `002`'s `fixtures/node-all-protocols.conf` are re-validated by this feature's test suite with the production type inventory registered; they must keep passing unchanged.
