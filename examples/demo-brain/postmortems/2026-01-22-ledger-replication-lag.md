---
type: postmortem
title: "Postmortem: 22 January ledger replication lag"
date: 2026-01-27
incident: 2026-01-22-ledger-replication-lag
written_by: luis-ferreira
root_cause_in: postgres-primary
contributing_decision: 0001-postgres-as-system-of-record
action_items: [151-alert-on-replication-lag]
---

Blameless review of [the 22 January incident](../incidents/2026-01-22-ledger-replication-lag.md).
Sev-2, 210 minutes, two customers shown a stale balance.

## Root cause

A schema migration held a lock on the [ledger](../systems/ledger.md) tables long
enough to stall WAL replay on the asynchronous replica of
[postgres-primary](../systems/postgres-primary.md). Reporting reads served from
that replica returned data up to 94 minutes old.

## Contributing factors

**A documented hazard with no enforcement.**
[ADR-0001](../decisions/0001-postgres-as-system-of-record.md) states plainly that
replicas and projections may lag and that read-your-writes is not guaranteed
against them. The reporting path had nonetheless drifted into being treated as
authoritative for customer-facing balances. The decision was correct and widely
known; nothing made it *checkable*.

**Replication lag was unmonitored.** Both replicas were monitored for liveness.
Neither was monitored for how far behind it was — the single number that would
have turned a three-hour investigation into an alert.

## What went well

Repointing reporting at the synchronous replica took 25 minutes from diagnosis
and required no schema or application change, because
[ADR-0001](../decisions/0001-postgres-as-system-of-record.md) had kept every
consumer honest about reading from a replica in the first place.

## Action items

- [#151](../issues/151-alert-on-replication-lag.md) — alert on replication lag in
  seconds, per replica, and make the reporting path's tolerance explicit.
