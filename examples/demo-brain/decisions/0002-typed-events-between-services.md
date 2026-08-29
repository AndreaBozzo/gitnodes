---
type: adr
title: "ADR-0002: Services talk over typed events"
status: accepted
date: 2024-09-30
affects: [checkout-service, notification-worker]
written_by: luis-ferreira
owned_by: payments
---

## Context

Services were calling each other synchronously over HTTP. A slow
[notification-worker](../systems/notification-worker.md) could add latency to a
checkout, and every new consumer meant another caller to coordinate with.

## Decision

Services publish typed, versioned events and consume them asynchronously.
Synchronous calls are reserved for the path where the user is waiting on the
result — checkout to [ledger](../systems/ledger.md), and nothing else.

## Consequences

- A slow consumer can no longer slow down a payment.
- Consumers may be arbitrarily late, so "late" needs an explicit budget per
  consumer. We did not set one, and
  [2 May](../incidents/2026-05-02-notification-backlog.md) is the bill for that.
- Every event needs an [idempotency key](../concepts/idempotency-key.md), because
  at-least-once delivery is the only delivery we get.
