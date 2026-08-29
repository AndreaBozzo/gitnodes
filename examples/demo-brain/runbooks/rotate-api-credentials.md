---
type: runbook
title: Rotate API gateway credentials
system: api-gateway
owned_by: platform
last_verified: 2026-05-20
status: current
---

## When to use

Scheduled quarterly rotation, or immediately on suspicion that a signing key or
upstream credential has leaked.

## Procedure

1. Issue the new key and publish it alongside the old one. The gateway accepts
   both during the overlap window.
2. Roll [api-gateway](../systems/api-gateway.md) instances. Because
   [ADR-0004](../decisions/0004-session-state-in-redis.md) made the gateway
   stateless, this is an ordinary rolling deploy and logs nobody out.
3. Wait one full session lifetime before revoking the old key.
4. Revoke, and confirm the authentication error rate stays flat.

## What this cannot do

Rotating the signing key does not invalidate sessions already issued under it
until the overlap window closes. Under
[ADR-0009](../decisions/0009-session-state-in-signed-cookies.md) that window is
what bounds how quickly a compromised session can be shut off.
