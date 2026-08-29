---
type: system
title: api-gateway
tier: 1
language: Go
owned_by: platform
depends_on: [session-store]
runbooks: [rotate-api-credentials]
governed_by: [0004-session-state-in-redis, 0009-session-state-in-signed-cookies]
tags: [featured, outage-2026-03-11]
---

Terminates TLS, authenticates every request, and fans out to the services behind
it. Tier 1: if the gateway is down, Harbor is down.

Since [ADR-0004](../decisions/0004-session-state-in-redis.md) the gateway has been
stateless, looking sessions up in [session-store](../systems/session-store.md) on
every request. That removed sticky routing from the load balancer and made deploys
boring — at the cost of turning a cache into a tier-1 dependency, which is the
trade [the 11 March outage](../incidents/2026-03-11-checkout-session-loss.md)
eventually collected on.

[ADR-0009](../decisions/0009-session-state-in-signed-cookies.md) is currently
walking that back.
