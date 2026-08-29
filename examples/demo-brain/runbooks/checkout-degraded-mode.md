---
type: runbook
title: Put checkout into degraded mode
system: checkout-service
owned_by: payments
last_verified: 2026-02-17
status: current
---

## When to use

[checkout-service](../systems/checkout-service.md) can still authorise payments
but a downstream dependency is failing — most often
[notification-worker](../systems/notification-worker.md) backing up, or
[ledger](../systems/ledger.md) writes timing out.

## Procedure

1. Set the `checkout.degraded` flag. Checkout stops waiting on receipt delivery
   and returns as soon as the authorisation and the ledger write have both
   committed.
2. Verify authorisations are still landing in the
   [ledger](../systems/ledger.md). If they are not, this is not a degradation —
   stop and declare an incident.
3. Watch the retry queue depth. Every deferred receipt carries its
   [idempotency key](../concepts/idempotency-key.md), so replay after recovery is
   safe and needs no deduplication pass.

## What this cannot do

Degraded mode protects *money*, not *experience*: customers are charged correctly
and told about it late. It is not a substitute for fixing the dependency, and
holding it for more than a few hours will exhaust the
[error budget](../concepts/error-budget.md) on receipt latency.
