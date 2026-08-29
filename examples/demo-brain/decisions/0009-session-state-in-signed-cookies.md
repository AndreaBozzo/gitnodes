---
type: adr
title: "ADR-0009: Session state moves to signed cookies"
status: accepted
date: 2026-04-28
supersedes: 0004-session-state-in-redis
affects: [api-gateway, session-store]
written_by: priya-raman
owned_by: platform
tags: [featured, outage-2026-03-11]
---

## Context

[ADR-0004](../decisions/0004-session-state-in-redis.md) made
[api-gateway](../systems/api-gateway.md) stateless by making it depend on a Redis
cluster for every request. [11 March](../incidents/2026-03-11-checkout-session-loss.md)
demonstrated what that dependency costs when the cluster behaves like the cache
it was configured to be.

Raised by [issue #145](../issues/145-revisit-session-state-strategy.md), filed out
of [the postmortem](../postmortems/2026-03-11-checkout-session-loss.md).

## Decision

Session state moves into a signed, encrypted cookie held by the client.
[session-store](../systems/session-store.md) is demoted back to a cache — for
rate-limit counters and revocation lists — and drops out of the tier-1 request
path.

Revocation, the one thing cookies cannot do, is handled by a short token lifetime
plus a small deny-list that the gateway may fail open on.

## Consequences

- Losing the Redis cluster degrades rate limiting instead of logging out every
  user. The [blast radius](../concepts/blast-radius.md) of a cache failure is a
  cache-shaped problem again.
- Sessions are capped at 4 KB and cannot be invalidated instantly. We are
  explicitly accepting up to 15 minutes of validity after logout.
- This ADR keeps what ADR-0004 omitted: the durability and failure requirements
  are stated here, not left to whoever provisions the infrastructure.
