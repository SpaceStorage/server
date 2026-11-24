# Contract: S3 Config Directives

**Feature**: `019-s3-protocol` | Spec FR-001, FR-012, FR-034, FR-060 | Extends `002` `config-directives.md`

## Entrypoint

```text
entrypoint s3 {
  address 0.0.0.0:9000;   # distinct port — required
  tls ... | plaintext;
}
```

Two handlers MUST NOT share `address:port` (`entrypoint_duplicate_address`).

## `protocols.s3` block

| Knob | Default | Meaning |
|------|---------|---------|
| `virtual_host_suffix` | unset | When set, accept `Host: {bucket}.{suffix}` |
| `region` | `us-east-1` | Location API constant; SigV4 accepts any region string |
| `multipart_idle` | `24h` | Abort incomplete uploads after idle (R7) |
| `multipart_min_part` | `5MiB` | Min intermediate part size |
| `multipart_max_parts` | `10000` | Max parts per upload |

## Limits interaction (`015`)

| Knob | Default | S3 check |
|------|---------|----------|
| `limits.max_key` | 1KiB | object key length |
| `limits.max_value` | 16MiB | assembled object / multipart sum |
| `limits.max_connections_per_entrypoint` | 10000 | accept on s3 port |
| `limits.max_connections_per_principal` | 1000 | after auth |

Suite A large-object and Suite B backup fixtures MUST raise `max_value` (and related caps) via a dedicated conf — defaults stay for ordinary smoke.

## Auth

Credentials are namespace-bound principals with SigV4 access key material from `014` (not inline in conf). TLS vs plaintext per entrypoint declaration.

## First binary

`ReleaseProfile::FirstBinary` forbids handler `s3`. Config that declares an `s3` entrypoint on a FirstBinary build MUST fail validation / refuse to start that listener (existing profile machinery).
