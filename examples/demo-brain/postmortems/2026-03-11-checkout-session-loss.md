---
type: postmortem
title: "Postmortem: 11 March checkout session loss"
date: 2026-03-18
incident: 2026-03-11-checkout-session-loss
written_by: dana-okonkwo
root_cause_in: session-store
contributing_decision: 0004-session-state-in-redis
action_items: [142-enable-session-store-persistence, 143-rewrite-session-store-failover-runbook, 144-alert-on-session-store-evictions, 145-revisit-session-state-strategy]
tags: [featured, outage-2026-03-11]
---

Blameless review of [the 11 March incident](../incidents/2026-03-11-checkout-session-loss.md).
96 minutes, sev-1, ~40% of active users logged out. No financial impact — every
write carried an [idempotency key](../concepts/idempotency-key.md), so the
retries that followed were absorbed correctly.

## Root cause

[session-store](../systems/session-store.md) was configured as a cache and
depended upon as a database.

[ADR-0004](../decisions/0004-session-state-in-redis.md) moved sessions into Redis
to make [api-gateway](../systems/api-gateway.md) stateless. It recorded the
benefit and named the new dependency, but it never stated what the cluster had to
guarantee in exchange — no durability requirement, no eviction policy, no
isolation from other workloads. Fourteen months later a reporting job filled the
same cluster and `allkeys-lru` did exactly what an LRU cache is designed to do.

The eviction was not a malfunction. Every component behaved as configured. The
defect was that no one component knew what the system as a whole had promised.

## Contributing factors

**The decision and the configuration were never connected.** The ADR lived with
the code; `maxmemory-policy` lived in an infrastructure repository. Neither
referenced the other, so promoting Redis from cache to tier-1 dependency changed
nothing about how it was provisioned.

**The runbook was eight months stale.** The August 2025 migration to managed
Redis removed the replica that step 1 of
[the failover runbook](../runbooks/session-store-failover.md) named.
[ADR-0007](../decisions/0007-runbooks-live-beside-systems.md) exists to catch
exactly this, but it binds runbooks to *systems*, and the change that invalidated
this one was made in a different repository that never touched the system note.
Twenty of the 96 minutes were spent discovering the runbook was wrong.

**The alert pointed at the symptom.** Paging on checkout failure rate sent
responders to [checkout-service](../systems/checkout-service.md), which was
healthy. Nothing alerted on the eviction counter, which was the actual signal.

## What went well

- Degraded auth on the gateway stopped new logouts in seven minutes once the
  cause was understood. [Graceful degradation](../concepts/graceful-degradation.md)
  was available because someone had built it before it was needed.
- [Idempotency keys](../concepts/idempotency-key.md) meant a session-layer failure
  stayed a session-layer failure. The [blast radius](../concepts/blast-radius.md)
  never reached the [ledger](../systems/ledger.md).

## Action items

- [#142](../issues/142-enable-session-store-persistence.md) — enable persistence
  and stop sharing the cluster with batch workloads.
- [#143](../issues/143-rewrite-session-store-failover-runbook.md) — rewrite the
  failover runbook against the managed-Redis topology.
- [#144](../issues/144-alert-on-session-store-evictions.md) — alert on the
  eviction counter, not on checkout's error rate.
- [#145](../issues/145-revisit-session-state-strategy.md) — reopen the ADR-0004
  trade-off entirely.

That last one is the important one. The first three make this failure survivable;
#145 asks whether the dependency should exist at all. It became
[ADR-0009](../decisions/0009-session-state-in-signed-cookies.md).
