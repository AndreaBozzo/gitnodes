---
type: runbook
title: Promote a PostgreSQL replica
system: postgres-primary
owned_by: platform
last_verified: 2026-06-11
status: current
---

## When to use

[postgres-primary](../systems/postgres-primary.md) is unreachable or has lost its
disk. Because [ADR-0001](../decisions/0001-postgres-as-system-of-record.md) makes
these tables the system of record, this is always at least a sev-2.

## Procedure

1. **Promote the synchronous replica, never the asynchronous one.** The
   asynchronous replica may be arbitrarily far behind — that is the whole finding
   of [22 January](../incidents/2026-01-22-ledger-replication-lag.md).
2. Confirm the promoted node's last ledger sequence number matches the last one
   acknowledged to [checkout-service](../systems/checkout-service.md).
3. Repoint the connection pooler. Applications reconnect on their own.
4. Rebuild projections afterwards. They are derived, so they may lag or be
   rebuilt freely; the ledger may not.

## What this cannot do

Promotion cannot recover writes that were never replicated. If the synchronous
replica is also gone, stop and escalate — restoring from a base backup is a
different, much longer procedure.
