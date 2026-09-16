# Config fixtures for `008` contract tests

Valid: [metrics-block.conf](metrics-block.conf) — slice-9 complete-product starter (global OTel off, scrape on admin-http only, sinks omitted).

Invalid (each file one `validate` failure):

| File | Expected code |
|------|----------------|
| `invalid/otel-on-first-binary.conf` | `ObservabilitySlice9Required` |
| `invalid/kafka-empty-brokers.conf` | `SinkConfigInvalid` |
| `invalid/slow-query-on-first-binary.conf` | `ObservabilitySlice9Required` |
