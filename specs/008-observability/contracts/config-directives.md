# Contract: Config directives

**Feature**: `008-observability` | Grammar: `001` | Reserved word `metrics` claimed here

## Node (global)

```text
metrics {
  otel {
    endpoint URL;          # slice 9; omit = no global OTel
    interval DURATION;     # default 15s
  }
  slow_query {
    enabled on|off;        # default off
    threshold DURATION;    # default 1s
  }
  audit_log on|off;        # default off; requires AUDIT_READ to turn on
}

log {
  level info;
  format json;
  kafka {                  # slice 9; omit = no global Kafka
    brokers HOST:PORT[,…];
    topic IDENT;
    tls { cert PATH; key PATH; ca PATH; } | plaintext;
  }
  syslog {                 # slice 9
    address HOST:PORT;
    transport udp|tcp;     # default udp
    tls { … } | plaintext; # tcp only
  }
}

entrypoint metrics_prom {
  port 9100;
  handler metrics;
  plaintext;
}
```

`log.level` / `log.format` remain `001`. Kafka/syslog nested blocks are additive.

First-binary profile: `otel`, `kafka`, `syslog`, `slow_query.enabled on`, `audit_log on`, `handler metrics` tenant paths → validate error `ObservabilitySlice9Required`.

## Namespace telemetry (cluster record; also expressible in config for starters)

```text
namespace acme {
  metrics {
    scrape on;
    otel { endpoint URL; }
  }
  log {
    kafka { brokers …; topic acme_logs; plaintext; }
    syslog { address …; transport tcp; plaintext; }
  }
}
```

Maps to `NamespaceTelemetry`. `007` `private_metrics`/`private_logs` true without slice 9 → `ObservabilitySlice9Required`.

## Reload

`slow_query.*`, sink enable/disable, OTel endpoint: **live** where `001` live class allows (re-read). `handler metrics` port: **restart**.
