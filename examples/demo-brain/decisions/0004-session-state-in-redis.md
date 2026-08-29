---
type: adr
title: "ADR-0004: Session state moves to Redis"
status: superseded
date: 2025-01-14
affects: [api-gateway, session-store]
written_by: priya-raman
owned_by: platform
tags: [featured, outage-2026-03-11]
---

## Context

Sessions lived in [api-gateway](../systems/api-gateway.md) process memory, so the
load balancer had to pin each user to one instance. Sticky routing made every
deploy a partial outage: draining an instance logged its users out, so deploys
happened at night, rarely, and in large risky batches.

## Decision

Move session state into a shared Redis cluster —
[session-store](../systems/session-store.md) — and make the gateway stateless.
The load balancer can then route any request to any instance.

## Consequences

- Deploys become routine and mid-day. This worked: deploy frequency went from
  weekly to roughly forty times a week.
- **The gateway gains a hard runtime dependency on Redis.** This ADR recorded
  that consequence and then said nothing about what Redis must therefore
  guarantee — no durability requirement, no eviction policy, no capacity model.

That omission is the direct cause of
[the 11 March checkout outage](../incidents/2026-03-11-checkout-session-loss.md).
The cluster kept the cache configuration it was provisioned with, and evicted
live sessions under memory pressure exactly as a cache is supposed to.

Superseded by
[ADR-0009](../decisions/0009-session-state-in-signed-cookies.md).
