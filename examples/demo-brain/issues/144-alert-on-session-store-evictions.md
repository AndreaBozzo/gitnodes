---
type: issue
title: "#144: Alert on session-store evictions, not checkout errors"
state: done
opened: 2026-03-18
closed: 2026-03-27
system: session-store
from_postmortem: 2026-03-11-checkout-session-loss
assignee: dana-okonkwo
tags: [outage-2026-03-11]
---

From [the 11 March postmortem](../postmortems/2026-03-11-checkout-session-loss.md).

The page fired on [checkout-service](../systems/checkout-service.md) error rate,
sending responders to a healthy system. The `evicted_keys` counter on
[session-store](../systems/session-store.md) had been climbing for three minutes
before checkout noticed, and nothing was watching it.

## Done

- Page on any non-zero sustained eviction rate on the session cluster.
- The alert links directly to
  [the failover runbook](../runbooks/session-store-failover.md).
- Checkout's error-rate alert now carries its upstream dependencies in the
  payload, so "checkout is down" arrives with the list of things that could
  actually be causing it.

Alerting on the symptom is not wrong — users experience symptoms. It is wrong as
the *only* alert.
