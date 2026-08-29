---
type: system
title: postgres-primary
tier: 1
language: PostgreSQL 16
owned_by: platform
runbooks: [postgres-failover]
governed_by: [0001-postgres-as-system-of-record]
---

The primary database, with one synchronous and one asynchronous replica. Holds
the [ledger](../systems/ledger.md) tables that
[ADR-0001](../decisions/0001-postgres-as-system-of-record.md) designates as
Harbor's system of record.

The asynchronous replica serves reporting reads. It fell far enough behind on
[22 January](../incidents/2026-01-22-ledger-replication-lag.md) to serve a
customer a balance that was ninety minutes stale.
