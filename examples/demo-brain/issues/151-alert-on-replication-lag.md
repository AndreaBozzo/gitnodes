---
type: issue
title: "#151: Alert on PostgreSQL replication lag"
state: in-progress
opened: 2026-01-27
system: postgres-primary
from_postmortem: 2026-01-22-ledger-replication-lag
assignee: luis-ferreira
---

From [the 22 January postmortem](../postmortems/2026-01-22-ledger-replication-lag.md).

Both replicas of [postgres-primary](../systems/postgres-primary.md) are monitored
for liveness and neither for lag, so a replica can be up, serving, and 94 minutes
stale without any signal.

## Scope

- Export replication lag in seconds, per replica.
- Page when the synchronous replica exceeds 5s; warn when the asynchronous
  replica exceeds 60s.
- Make the reporting path's staleness tolerance explicit, so
  [ADR-0001](../decisions/0001-postgres-as-system-of-record.md)'s "replicas may
  lag" becomes a number a consumer can check rather than a caveat people
  remember.

## Open question

The third bullet is the one that matters and the one that is stuck. A tolerance
per *replica* is easy; a tolerance per *consumer* means every reader declaring
what staleness it can accept, and that is a larger change than an alert.
