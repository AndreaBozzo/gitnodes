---
type: concept
title: Error budget
---

The amount of unreliability a system is allowed before reliability work takes
priority over feature work.

Owned by [SRE](../teams/sre.md), spent by whoever owns the service. The budget is
not a target to hit — it is a decision procedure. While there is budget left, ship
features; when it is exhausted, stop and fix things.

Two failures in this repository show the budget's limits:

- [11 March](../incidents/2026-03-11-checkout-session-loss.md) consumed a quarter
  of the availability budget in 96 minutes and produced four action items. The
  budget worked as intended.
- [2 May](../incidents/2026-05-02-notification-backlog.md) consumed *none* of it,
  because [notification-worker](../systems/notification-worker.md) has no latency
  objective to spend against. Eleven hours of undelivered receipts registered as
  zero. See [#158](../issues/158-bound-notification-lateness.md).

A budget only governs what it measures. An unmeasured system is not within
budget — it is outside the system entirely.
