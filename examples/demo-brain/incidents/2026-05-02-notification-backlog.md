---
type: incident
title: "2026-05-02: notification backlog"
severity: sev-3
date: 2026-05-02
duration_minutes: 660
affected: [notification-worker]
commander: sam-whitfield
---

[notification-worker](../systems/notification-worker.md) stopped consuming after
a poison message and accumulated an eleven-hour backlog of receipts and dunning
mail. No payments were affected; roughly 30,000 receipts arrived late.

## Timeline (UTC)

- **02:10** — A malformed event from an old producer version fails
  deserialisation. The consumer retries it forever instead of dead-lettering.
- **02:10–13:00** — Queue depth climbs steadily. There is no alert, because a
  tier-3 consumer being late is explicitly acceptable under
  [ADR-0002](../decisions/0002-typed-events-between-services.md).
- **13:04** — A customer asks why yesterday's receipt has not arrived.
- **13:20** — The poison message is dead-lettered by hand and the consumer
  drains.

## Immediate cause

No dead-letter path for events that fail deserialisation.

## Why nobody noticed for eleven hours

[ADR-0002](../decisions/0002-typed-events-between-services.md) says consumers
"may be arbitrarily late" and never converts that into a number. "Allowed to be
late" without a bound is indistinguishable from "allowed to be stopped".

No postmortem was written — a sev-3 does not require one under Harbor's process.
The absence is deliberate, and it is why
[issue #158](../issues/158-bound-notification-lateness.md) had to be filed
directly rather than falling out of an analysis.
