# Contract: `admin` handler wire protocol (TCP)

**Feature**: `001-runtime-cli-api` | Codec in `crates/admin-proto/src/frame.rs`

## Framing

```text
frame := u32_be(len) payload[len]
len   := byte length of payload, 1 ..= 4_194_304 (4 MiB)
payload := UTF-8 JSON object
```

Oversized `len` → server replies `error{code:"protocol_version", message:"frame too large"}` and closes. Non-JSON payload → `error{code:"internal"}` and close.

## Handshake

Client's first frame MUST be:

```json
{ "hello": { "proto_version": 1, "token": "<bearer>", "client": "spacestorage-cli/0.1.0" } }
```

Server replies with one of:

```json
{ "hello_ok": { "proto_version": 1, "node_name": "db-1", "state": "ready" } }
{ "error": { "code": "unauthorized", "message": "invalid admin token" } }          // then close
{ "error": { "code": "protocol_version", "message": "supported: [1]" } }          // then close
```

The handshake reads at most one frame; a client that sends anything else first is closed after the error frame. Handshake must complete within 5 s or the server closes the connection (protects against slowloris on the admin port).

## Requests and responses

```json
{ "id": 7, "op": "status", "args": {} }
{ "id": 7, "ok": true,  "result": { ...status... } }
{ "id": 8, "ok": false, "error": { "code": "invalid_state", "message": "...", "details": { "state": "draining" } } }
```

- `id` is client-chosen and echoed; requests MAY be pipelined; responses are returned in completion order.
- `op` ∈ `status | config | threads | buffers | reload | stop`; anything else → `unknown_op`.
- `stop` with `args.wait=true` delays the response until exit/timeout; connection EOF after the frame is normal.
- Idle connections are closed after 10 minutes without a frame.

## TLS

When the entrypoint declares `tls`, the whole stream is TLS from byte 0 (no STARTTLS). Plaintext bytes on a TLS entrypoint fail the handshake and are dropped (SC-006).

## Buffers used

`net.recv` reserves `len` bytes per inbound frame for the frame's lifetime; `net.send` reserves the serialized response length until written. Overflow under `wait` policy stalls that connection only.
