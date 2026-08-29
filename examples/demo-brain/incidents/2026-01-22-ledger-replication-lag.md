---
type: incident
title: "2026-01-22: ledger replication lag"
severity: sev-2
date: 2026-01-22
duration_minutes: 210
affected: [postgres-primary, ledger]
runbook_used: postgres-failover
commander: dana-okonkwo
postmortem: 2026-01-22-ledger-replication-lag
---

The asynchronous replica of [postgres-primary](../systems/postgres-primary.md)
fell up to 94 minutes behind. Reporting reads served stale
[ledger](../systems/ledger.md) balances; two customers were shown a balance that
omitted a settled payment.

## Timeline (UTC)

- **13:05** — A schema migration takes a long-lived lock on the ledger tables.
- **13:20** — WAL replay on the asynchronous replica stalls behind the lock.
  Replication lag begins climbing. No alert exists on lag itself.
- **14:40** — A customer support ticket reports a "missing" payment.
- **15:10** — Investigation finds the payment present on the primary and absent
  on the replica serving reporting.
- **15:35** — Reporting reads are repointed to the synchronous replica.
- **16:35** — The migration completes, replay catches up, reads are repointed
  back.

## Immediate cause

A migration held a lock long enough to stall WAL replay, and nothing measured or
alerted on the resulting lag.

## Why it was customer-visible

[ADR-0001](../decisions/0001-postgres-as-system-of-record.md) is explicit that
projections and replicas may lag, but the reporting path had quietly come to be
treated as read-your-writes. The decision named the hazard; nothing enforced it.
