---
type: adr
topic: Use Git as the source of truth
date: 2026-01-02
status: accepted
decides_on: concepts/knowledge-graph.md
tags: [architecture]
---

# ADR 0001: Use Git as the source of truth

## Context

We want a knowledge base that is portable, diffable, and outlives any single
tool. Storing notes in a database would lock the content inside the app.

## Decision

Keep every note as markdown in a Git repository. The app is a lens over the
repo, never the system of record. Any index it builds can be rebuilt from a
clean clone.

## Consequences

- No lock-in: the repo is useful with or without GitNodes.
- History, review, and access control come from Git and the forge for free.
- The `decides_on:` field above links this decision to the
  [Knowledge graph](../concepts/knowledge-graph.md) concept as a typed edge.
