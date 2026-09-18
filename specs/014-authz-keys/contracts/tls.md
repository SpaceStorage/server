# Contract: Transport (TLS / plaintext / optional mTLS)

**Feature**: `014-authz-keys` | Crates: `config`, `node`, `authz` | Spec: FR-005 | Research R6 | Grammar: `001`

This feature does **not** replace [`001` config-grammar](../../001-runtime-cli-api/contracts/config-grammar.md) `tls { certificate; key; }` / `plaintext;`. Omitted transport remains a startup error (`TransportOmitted` / `001` FR-027). TLS-declared entrypoints refuse plaintext; no silent fallback.

## Optional mTLS

`001` reserved `client_ca`. This feature defines it:

```nginx
entrypoint {
    port 5432;
    handler postgresql;
    tls {
        certificate /etc/spacestorage/tls/server.crt;
        key         /etc/spacestorage/tls/server.key;
        client_ca   /etc/spacestorage/tls/clients.pem;   # optional
        # mtls_replace_password;                         # optional; default off
    }
}
```

When `client_ca` is set, the handshake MUST present a client cert signed by that CA. Map CN (else first DNS SAN) to principal **login**. Unknown login → AUTH fail, audit `auth.fail`. Default: mTLS is **in addition** to password/SCRAM. `mtls_replace_password` on that entrypoint: cert map is sufficient. Certificate material referenced, never inlined (`KeyMaterialForbidden`).

## Internode

`012` already requires `tls` or `plaintext;` on `internode`/`replication`. Peers still authenticate as `replication` + join secret; mTLS MAY pin node certs but MUST NOT replace the join secret in the first binary.
