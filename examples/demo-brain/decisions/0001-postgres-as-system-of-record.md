---
type: adr
title: "ADR-0001: PostgreSQL is the system of record"
status: accepted
date: 2024-02-19
affects: [ledger, postgres-primary]
written_by: priya-raman
owned_by: platform
---

## Context

Harbor was running three stores that each believed they were authoritative: the
Postgres tables, a search index, and a reporting warehouse. Reconciling them was a
weekly manual job, and disagreements were resolved by whoever complained loudest.

## Decision

The [ledger](../systems/ledger.md) tables in
[postgres-primary](../systems/postgres-primary.md) are the single system of
record. Every other store is a **projection**: it may be deleted at any time and
rebuilt from Postgres, and nothing is allowed to write to a projection and the
ledger in the same operation.

## Consequences

- Rebuilding a projection is a routine operation, not an incident.
- Postgres availability becomes the ceiling on Harbor's availability, which is
  why [postgres-failover](../runbooks/postgres-failover.md) is the most rehearsed
  procedure we have.
- Read-your-writes is not guaranteed against replicas — a gap that
  [22 January](../incidents/2026-01-22-ledger-replication-lag.md) turned into a
  customer-visible wrong balance.
