---
type: concept
title: Idempotency key
---

A caller-supplied identifier that makes a retried write indistinguishable from
the original.

[checkout-service](../systems/checkout-service.md) requires one on every payment,
and [ADR-0002](../decisions/0002-typed-events-between-services.md) requires one on
every event, because at-least-once delivery is the only delivery guarantee Harbor
has. The [ledger](../systems/ledger.md) stores the key alongside the entry and
returns the original result for a duplicate rather than writing a second one.

This is the least glamorous note in this repository and the reason
[11 March](../incidents/2026-03-11-checkout-session-loss.md) was a session
incident instead of a financial one. Forty percent of users were logged out
mid-checkout and retried. Every one of those retries carried the key from the
first attempt, so nobody was charged twice.

The [blast radius](../concepts/blast-radius.md) of a failure is bounded by the
weakest guarantee at its edge. Idempotency keys are how that edge is made strong
before anyone knows which failure will arrive.
