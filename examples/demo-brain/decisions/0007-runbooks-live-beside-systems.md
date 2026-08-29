---
type: adr
title: "ADR-0007: Runbooks live beside the systems they operate"
status: accepted
date: 2025-06-03
written_by: dana-okonkwo
owned_by: sre
---

## Context

Runbooks lived in a wiki nobody edited. They were written once, at the moment a
system launched, and then quietly diverged from reality. Nobody could answer "is
this procedure still true?" without executing it — usually during an incident.

## Decision

A runbook is a note in this repository, linked from the
[system](../systems/api-gateway.md) it operates and owned by a team. Changing a
system's topology means changing its runbooks in the same pull request.

## Consequences

- A reviewer can see, in one diff, that a change invalidates a procedure.
- Stale runbooks become visible as graph structure rather than as folklore: a
  runbook whose linked system changed underneath it is a query, not a hunch.
- It does not enforce itself. On
  [11 March](../incidents/2026-03-11-checkout-session-loss.md) the
  [failover runbook](../runbooks/session-store-failover.md) was eight months
  stale despite this ADR, because the change that invalidated it touched
  infrastructure config in a different repository.
