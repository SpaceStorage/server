# Contract: OpenTelemetry push

**Feature**: `008-observability` | Spec: FR-016 | Transport: OTLP/HTTP protobuf | Slice: 9

## Global

Node config `metrics { otel { endpoint URL; } }`. If omitted, no global push. Interval default `15s` (`interval` directive).

POST `{endpoint}/v1/metrics` (standard OTLP/HTTP). Resource attributes: `service.name=spacestorage`, `service.instance.id=<node uuid>`, `service.namespace=spacestorage`.

Payload is the **same series set** as global `/metrics` (including omit-label rules). Tenant-only endpoints are **not** mixed into the global push.

## Per-namespace

`NamespaceTelemetry.otel_endpoint` set → push **filtered** series (`namespace=<name>` only), same as tenant scrape. A namespace MAY enable scrape, OTel, or both (clarify Q3). Neither required.

## Failure

OTLP errors increment `spacestorage_otel_export_errors_total{stream}`. Drop the batch; do not block recorders. Default-channel log line on repeated failure.

## Not required

OTLP/gRPC, stdout exporter, Prometheus remote-write.
