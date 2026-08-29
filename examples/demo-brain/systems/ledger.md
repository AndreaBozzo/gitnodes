---
type: system
title: ledger
tier: 1
language: Kotlin
owned_by: payments
depends_on: [postgres-primary]
runbooks: [postgres-failover]
governed_by: [0001-postgres-as-system-of-record]
---

Append-only double-entry record of every movement of money. Per
[ADR-0001](../decisions/0001-postgres-as-system-of-record.md) the ledger tables in
[postgres-primary](../systems/postgres-primary.md) are the system of record;
everything else in Harbor is a projection that can be rebuilt.

The ledger is the reason Harbor can survive losing most things. It is also the
reason [replication lag](../incidents/2026-01-22-ledger-replication-lag.md) is an
incident rather than a metric.
