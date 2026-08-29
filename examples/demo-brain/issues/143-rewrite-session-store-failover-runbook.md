---
type: issue
title: "#143: Rewrite the session-store failover runbook"
state: done
opened: 2026-03-18
closed: 2026-03-24
system: session-store
from_postmortem: 2026-03-11-checkout-session-loss
assignee: sam-whitfield
tags: [outage-2026-03-11]
---

From [the 11 March postmortem](../postmortems/2026-03-11-checkout-session-loss.md).

[The failover runbook](../runbooks/session-store-failover.md) named a replica
removed in August 2025. Twenty minutes of a sev-1 went into discovering that the
procedure, not the system, was the thing that was broken.

## Done

- Rewritten against the managed-Redis topology actually in production.
- Added the eviction-rate check as step 1, since that is the signal that
  distinguishes this failure from an ordinary outage.
- Added an explicit "what this cannot do" section. The old runbook implied
  failover would recover evicted sessions. It cannot, and believing otherwise
  cost time.

## Follow-up

[ADR-0007](../decisions/0007-runbooks-live-beside-systems.md) did not catch this,
because the invalidating change was made in the infrastructure repository and
never touched the system note here. Making that binding enforceable is a real
open question, not something this issue closed.
