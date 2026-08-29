---
type: concept
title: Blast radius
tags: [featured]
---

How much of Harbor a single failure can reach.

Blast radius is a property of *dependencies*, not of code quality. A perfectly
written service that everything depends on has a large blast radius; a badly
written tier-3 consumer has a small one.

[ADR-0004](../decisions/0004-session-state-in-redis.md) is the clearest example in
this repository of the number changing without anyone deciding to change it.
Before it, losing the Redis cache degraded performance. After it, losing the same
cluster logged out every user — the cluster did not change, its blast radius did.
[The 11 March outage](../incidents/2026-03-11-checkout-session-loss.md) is what
collecting on that looks like, and
[ADR-0009](../decisions/0009-session-state-in-signed-cookies.md) is the work of
shrinking it back.

The practical question to ask of any decision: **what is now broken that was not
broken before, if this dependency disappears?** If the answer is not in the
decision record, the decision is not finished.
