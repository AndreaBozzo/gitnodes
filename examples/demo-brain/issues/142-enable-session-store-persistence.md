---
type: issue
title: "#142: Enable session-store persistence and isolate batch workloads"
state: done
opened: 2026-03-18
closed: 2026-04-02
system: session-store
from_postmortem: 2026-03-11-checkout-session-loss
assignee: priya-raman
tags: [outage-2026-03-11]
---

From [the 11 March postmortem](../postmortems/2026-03-11-checkout-session-loss.md).

[session-store](../systems/session-store.md) runs with `maxmemory-policy
allkeys-lru`, no AOF, and no RDB — the configuration it was given when it was a
cache, unchanged by
[ADR-0004](../decisions/0004-session-state-in-redis.md) promoting it to a tier-1
dependency.

## Done

- `maxmemory-policy` set to `volatile-lru`, so only keys with an explicit TTL are
  eligible for eviction. Session keys carry no TTL of their own.
- AOF enabled with `everysec` fsync. A cluster restart no longer logs everyone
  out.
- Batch and reporting workloads moved to a separate cluster. The 11 March trigger
  cannot recur on this one.

## Deliberately not done

Persistence makes the failure survivable; it does not remove the dependency. That
question is [#145](../issues/145-revisit-session-state-strategy.md).
