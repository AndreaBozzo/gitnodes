---
type: issue
title: "#158: Give notification lateness a number"
state: todo
opened: 2026-05-02
system: notification-worker
assignee: sam-whitfield
---

From [the 2 May backlog incident](../incidents/2026-05-02-notification-backlog.md),
filed directly — a sev-3 does not get a postmortem under Harbor's process, so
there is no analysis note behind this one.

[ADR-0002](../decisions/0002-typed-events-between-services.md) says consumers
"may be arbitrarily late". [notification-worker](../systems/notification-worker.md)
took that literally and was stopped for eleven hours without a single alert,
because nothing distinguishes "late" from "dead" when late has no bound.

## Scope

- Dead-letter events that fail deserialisation instead of retrying them forever.
- Set an explicit lateness budget for the consumer and alert on it.
- Amend [ADR-0002](../decisions/0002-typed-events-between-services.md) so
  "arbitrarily late" is stated as a per-consumer budget rather than an absence of
  one.

The third item is the real fix. The first two stop this recurrence; the amendment
stops the next consumer from inheriting the same gap.
