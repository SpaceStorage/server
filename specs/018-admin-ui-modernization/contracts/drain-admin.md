# Contract: Drain and undrain on the admin path

**Feature**: `018-admin-ui-modernization` | Semantics: `011` `crates/membership/src/drain.rs` | Op: `crates/admin-proto`

`011` remains the only writer of membership events. These ops are the admin-http entry the shell and the CLI share. They do not decommission or replace a node.

## Operations

`POST /v1/admin`:

```json
{ "op": "drain", "node_id": "018f2c6e-7b1a-7c3d-8e4f-1a2b3c4d5e6f" }
```

```json
{ "op": "undrain", "node_id": "018f2c6e-7b1a-7c3d-8e4f-1a2b3c4d5e6f" }
```

The handler calls `MembershipService::operator_drain` or `operator_undrain`, which:

1. Loads the current membership view.
2. Calls `membership::drain` or `membership::undrain`.
3. Persists the returned `MembershipEvent::Drain` or `Undrain` through the membership event path `011` already uses.

A UI crate must not set `MemberRecord.status` itself.

Authorization: `CLUSTER_ADMIN` only. Anyone else, including a namespace admin, receives **403** `forbidden` and the membership view is unchanged. Cookie callers also pass [session-cookie.md](session-cookie.md). The admin-token bearer does not need CSRF.

## Results

Success **200**:

```json
{
  "node_id": "018f2c6e-7b1a-7c3d-8e4f-1a2b3c4d5e6f",
  "status": "draining"
}
```

Undrain returns `"status": "ready"`.

Not a member → **400** with the `011` error code (`NotMember` today) and `message` from that error. The shell shows `message` and does not change the node row to draining.

Drain is not cluster stop. `POST /v1/stop` remains the `001` stop path (`exit_after_drain` on that path stays as `001` defined it). The shell's cluster-stop control calls `/v1/stop`, not `op: drain`.

## Shell confirmation (client behavior the acceptance suite asserts)

Drain (`strength: strong`):

- The dialog names the selected node's name and id.
- The effect text states that the node stops accepting new tenant connections and new placements, and that replicas move off before decommission.
- The operator must type the node name. The input starts empty.
- Dismiss sends no `POST`.
- A forced `POST` without `CLUSTER_ADMIN` is 403 even if the button was shown.

Undrain (`strength: normal`):

- The dialog names the node and states that it may accept placements again.
- Dismiss sends no `POST`.

Cluster stop is specified here only so the suite does not confuse it with drain. `POST /v1/stop` confirmation names the whole cluster, states that every service on it stops, and requires the operator to type `cluster`. The input starts empty.

## Tests

- Drain as cluster admin moves the targeted member to `draining` and leaves the process up (existing `011` drain contract).
- Undrain returns that member to `ready`.
- Namespace-admin cookie → 403, status unchanged.
- Cookie drain without `Origin` or with a bad CSRF token → 403 `csrf_rejected`, status unchanged.
- Dialog markup for drain includes the node name and the impact sentence, and the acknowledgement input's `value` is empty.
