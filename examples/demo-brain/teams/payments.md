---
type: team
title: Payments
lead: luis-ferreira
slack: "#team-payments"
---

Owns [checkout-service](../systems/checkout-service.md) and the
[ledger](../systems/ledger.md). Payments carries the only systems at Harbor where
[blast radius](../concepts/blast-radius.md) is measured in money rather than
latency, so it defaults to [graceful degradation](../concepts/graceful-degradation.md)
over hard failure.
