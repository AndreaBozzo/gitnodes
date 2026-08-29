---
type: person
title: Luis Ferreira
role: Senior Engineer, Payments
team: payments
---

Luis maintains [checkout-service](../systems/checkout-service.md) and the
[ledger](../systems/ledger.md) write path. He is unusually strict about
[idempotency keys](../concepts/idempotency-key.md), for reasons the
[11 March postmortem](../postmortems/2026-03-11-checkout-session-loss.md) makes
obvious: they are the only reason the outage cost sessions instead of money.
