---
type: runbook
title: Fail over the session store
system: session-store
owned_by: platform
last_verified: 2025-07-08
status: rewritten
tags: [outage-2026-03-11]
---

> **This runbook was wrong on 11 March 2026.** Step 1 pointed at a replica that
> had been removed in August 2025 when the cluster moved to managed Redis. It has
> since been rewritten under
> [issue #143](../issues/143-rewrite-session-store-failover-runbook.md); the
> history is kept here because
> [the postmortem](../postmortems/2026-03-11-checkout-session-loss.md) is about
> how it went stale, not just about Redis.

## When to use

[session-store](../systems/session-store.md) is unreachable, or evicting keys
faster than sessions are being created.

## Procedure

1. Confirm the eviction rate: `redis-cli info stats | grep evicted_keys`. A
   non-zero and climbing counter means the cluster is shedding live sessions, not
   just cold ones.
2. Raise `maxmemory` if there is headroom on the node. This buys minutes, not a
   fix.
3. If evictions continue, put [api-gateway](../systems/api-gateway.md) into
   **degraded auth**: the gateway accepts an unexpired token without a
   session-store lookup. Sessions stop being revocable; log-outs will not take
   effect until this is lifted.
4. Page the [Platform](../teams/platform.md) on-call. Do not restart the cluster
   — a restart discards every session it still holds.

## What this cannot do

Nothing here recovers sessions already evicted. Those users are logged out, and
the only remedy is that they log in again. If that is unacceptable, the fix is
durability, not failover — see
[issue #142](../issues/142-enable-session-store-persistence.md).
