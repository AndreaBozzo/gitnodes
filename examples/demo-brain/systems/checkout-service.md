---
type: system
title: checkout-service
tier: 1
language: Kotlin
owned_by: payments
depends_on: [api-gateway, ledger]
runbooks: [checkout-degraded-mode]
governed_by: [0002-typed-events-between-services]
tags: [featured, outage-2026-03-11]
---

Turns a cart into an authorised payment and a [ledger](../systems/ledger.md)
entry. Every write carries an [idempotency key](../concepts/idempotency-key.md),
so a retried checkout cannot double-charge.

Checkout is the most visible thing Harbor does, which means it is where other
systems' failures get noticed first — on
[11 March](../incidents/2026-03-11-checkout-session-loss.md) it was reported as a
checkout outage for forty minutes before anyone looked at
[session-store](../systems/session-store.md).
