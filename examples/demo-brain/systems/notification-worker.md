---
type: system
title: notification-worker
tier: 3
language: Python
owned_by: platform
depends_on: [ledger]
governed_by: [0002-typed-events-between-services]
---

Consumes ledger events and sends receipts, dunning mail, and webhooks to
customers. Tier 3: it may be hours late without anyone paging.

That tolerance is deliberate — see
[graceful degradation](../concepts/graceful-degradation.md) — but it has a floor.
On [2 May](../incidents/2026-05-02-notification-backlog.md) the queue grew for
eleven hours before anyone noticed, because "allowed to be late" had never been
given a number.
