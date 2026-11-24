# S3 Protocol Fixtures

Fixtures for Suite A / config validation land during implement. Expected set:

| Fixture | Purpose |
|---------|---------|
| `s3-entrypoint.conf` | Minimal node with distinct `s3` port, plaintext or TLS, namespace-bound keys |
| `s3-limits-raised.conf` | Raises `max_value` (and related) for large-object / Suite B |
| `s3-vhost.conf` | Sets `protocols.s3.virtual_host_suffix` |
| SigV4 vector files | Canonical request / string-to-sign / signature hex (unit + Suite A) |
| Object bodies | empty, sub-5MiB, multipart threshold, large streaming fixture + SHA256 sidecars |

Invalid conf cases (duplicate port, missing tls|plaintext) reuse `002` fixture patterns where applicable.
