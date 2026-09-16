# Contract: Metadata reads and writes

**Feature**: `006-control-plane` | Spec: FR-008, FR-011, SC-007

## Data path

Any **member** (voter or not, primary or not) coordinates tenant queries (`FR-008`). No extra hop to the cluster primary.

## Metadata writes

Create namespace, admit join (after `011` auth), schema/DDL, lease grant: must be **committed by the group's primary**. Submitted to a secondary: `RaftForward` or `NotLeader { leader }`. Never applied only on the secondary.

## Metadata reads

`describe cluster`, list namespaces, list schemas: any voter (primary or secondary) after **read-index** so the sample matches the primary (SC-007). Learners MAY serve the same after apply lag is zero; if apply lag > 0 they MUST wait or redirect rather than return a stale list as complete.

## Errors

| Code | Client sees |
|------|-------------|
| `NotLeader` | retry on `leader` or any member (forward) |
| `Minority` | unavailable for metadata mutation |
