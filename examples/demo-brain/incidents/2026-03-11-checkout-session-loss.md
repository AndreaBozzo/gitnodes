---
type: incident
title: "2026-03-11: checkout session loss"
severity: sev-1
date: 2026-03-11
duration_minutes: 96
affected: [session-store, api-gateway, checkout-service]
runbook_used: session-store-failover
commander: dana-okonkwo
postmortem: 2026-03-11-checkout-session-loss
tags: [featured, outage-2026-03-11]
---

Roughly 40% of active users were logged out mid-session over 96 minutes. Users
in [checkout](../systems/checkout-service.md) lost their cart at the payment step
and were bounced to a login screen. No money moved incorrectly.

## Timeline (UTC)

- **08:42** — A batch reporting job writes ~2 GB of cached query results into
  [session-store](../systems/session-store.md), sharing the cluster because it was
  provisioned as a general-purpose cache.
- **08:44** — Memory pressure hits `maxmemory`. The `allkeys-lru` policy starts
  evicting — and it does not distinguish reporting keys from live sessions.
- **08:47** — Checkout failure rate crosses alert threshold. Paged as *"checkout
  down"*, because that is where users noticed.
- **08:51** — [Sam](../people/sam-whitfield.md) opens the
  [session-store failover runbook](../runbooks/session-store-failover.md).
  Step 1 names a replica that has not existed since the August 2025 move to
  managed Redis. Twenty minutes go into establishing that the runbook is wrong
  rather than that Redis is.
- **09:14** — [Dana](../people/dana-okonkwo.md) takes command and calls it as a
  session-store incident, not a checkout incident.
- **09:31** — Degraded auth is enabled on [api-gateway](../systems/api-gateway.md):
  unexpired tokens are accepted without a lookup. New logouts stop immediately.
- **09:38** — The reporting job is killed and its keys are dropped.
- **10:18** — Eviction rate returns to zero, degraded auth is lifted, incident
  closed.

## Immediate cause

A cache shared between a batch job and live sessions, evicting under an LRU
policy that was correct for a cache and catastrophic for a session store.

## Why it took 96 minutes

Two separate failures of knowledge, both of which this repository is meant to
address:

1. Nothing connected [ADR-0004](../decisions/0004-session-state-in-redis.md) —
   which made sessions a tier-1 dependency — to the configuration of the cluster
   that held them.
2. Nothing connected the August 2025 infrastructure change to the
   [runbook](../runbooks/session-store-failover.md) it invalidated, despite
   [ADR-0007](../decisions/0007-runbooks-live-beside-systems.md) existing
   precisely to prevent that.

Analysis in [the postmortem](../postmortems/2026-03-11-checkout-session-loss.md).
