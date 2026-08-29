---
type: issue
title: "#145: Revisit the session-state strategy"
state: done
opened: 2026-03-18
closed: 2026-04-28
system: api-gateway
from_postmortem: 2026-03-11-checkout-session-loss
assignee: priya-raman
resulted_in: 0009-session-state-in-signed-cookies
tags: [featured, outage-2026-03-11]
---

From [the 11 March postmortem](../postmortems/2026-03-11-checkout-session-loss.md).

[#142](../issues/142-enable-session-store-persistence.md),
[#143](../issues/143-rewrite-session-store-failover-runbook.md) and
[#144](../issues/144-alert-on-session-store-evictions.md) make this failure mode
survivable and visible. None of them ask the harder question:
[ADR-0004](../decisions/0004-session-state-in-redis.md) bought deploy safety by
putting a network hop in front of every authenticated request. Was that the right
trade, and is it still?

## Outcome

It was the right trade in 2025 and it is not now. The load-balancer stickiness
ADR-0004 was solving no longer exists, and the [blast
radius](../concepts/blast-radius.md) of the dependency it introduced is every
logged-in user.

Resolved as [ADR-0009](../decisions/0009-session-state-in-signed-cookies.md):
session state moves into signed cookies and
[session-store](../systems/session-store.md) returns to being a cache.

Note what this required: reading a 2025 decision, a 2026 incident, and the
postmortem between them as one connected thing. That reconstruction is what this
repository exists to make cheap.
