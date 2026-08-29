---
type: system
title: session-store
tier: 1
language: Redis 7
owned_by: platform
runbooks: [session-store-failover]
governed_by: [0004-session-state-in-redis]
tags: [outage-2026-03-11]
---

A single Redis cluster holding every live session for
[api-gateway](../systems/api-gateway.md).

It is labelled tier 1 today, but it was provisioned as a cache — `maxmemory-policy
allkeys-lru`, no AOF, no RDB. Nothing re-examined those settings when
[ADR-0004](../decisions/0004-session-state-in-redis.md) promoted it from
"nice-to-have cache" to "the thing that decides whether you are logged in".

That gap between what a system is configured as and what it is depended on for is
the whole story of [11 March](../incidents/2026-03-11-checkout-session-loss.md).
