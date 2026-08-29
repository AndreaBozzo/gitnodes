---
type: concept
title: Graceful degradation
---

Failing in a way that loses the least valuable thing first.

Harbor's rule: money is protected before experience, and experience before
convenience. [checkout-service](../systems/checkout-service.md) will take a
payment and send the receipt late; it will never send the receipt and lose the
payment.

Degradation has to be built before it is needed. On
[11 March](../incidents/2026-03-11-checkout-session-loss.md) the degraded-auth
mode on [api-gateway](../systems/api-gateway.md) stopped the bleeding seven
minutes after the cause was understood — but only because someone had implemented
it months earlier, when there was time to reason about what it was safe to give
up. Nobody designs a good failure mode at 09:31 during a sev-1.

The counterpart is that every degraded mode is a promise you have quietly broken.
[Degraded auth](../runbooks/session-store-failover.md) means logouts stop taking
effect; that is acceptable for an hour and not for a day.
