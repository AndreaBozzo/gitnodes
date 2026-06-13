# GitNodes roadmap

The overall direction, not a task tracker. Dates are intentionally absent;
GitNodes grows demand-first — a slice ships when real usage justifies it, not on
a schedule.

## Principles

- **Git is the source of truth.** SQLite is a rebuildable projection and nothing
  more. Anything that can't survive `git clone` alone is a cache, never a store.
- **No lock-in.** Your knowledge base is plain markdown with YAML frontmatter in
  your own repository. GitNodes is a lens over it, not a database you check into.
- **Merge stays where it belongs.** Concurrent-edit resolution is offloaded to
  GitHub pull requests by design rather than reimplemented in-app.
- **Demand-driven scope.** The list below is direction, not commitment. Features
  graduate from "exploring" only when a concrete use case pulls them in.

## Stable today

The core is mature and in daily use:

- Config-driven node types and typed graph edges (`.gitnodes.yml`).
- Atomic multi-file commits over the GitHub Git Data API, with preconditions and
  retry.
- Permission-aware writes: direct commit with `push`, automatic pull-request
  fallback otherwise.
- Rebuildable SQLite projection with full-text search (FTS5).
- Multi-repository routing, one deployment serving many targets.
- Inbound sync via HMAC-verified webhooks; live freshness over SSE.
- Bidirectional work-item ↔ GitHub issue/PR binding.
- Security and operational hardening: CSRF protection, rate limiting, session
  encryption, `/healthz` and `/readyz`.

## In progress

- **Write-path unification** — converging every mutation (save, delete, rename,
  config, assets, work items) onto the single atomic transaction layer, so all
  write paths share the same precondition and rollback guarantees.
- **Open-source readiness** — clean public packaging, documentation, and
  license/policy clarity.

## Exploring (tracked, not committed)

Pulled in only when a real need appears:

- Richer conflict resolution and an in-app review surface.
- Advisory locks to reduce avoidable write conflicts.
- An activity stream over the audit log.
- External blob storage for binary assets (so large files don't bloat the repo).
- Forge abstraction beyond GitHub (GitLab, Gitea, self-hosted).
- Temporal / history views of the graph.
- Local / offline mode.
- Large-graph performance: viewport culling past the current ~500-node comfort
  zone.

## Known limitations

See [OPERATOR_NOTES.md](OPERATOR_NOTES.md#what-it-does-not-promise) for the
current accepted limitations and the failure-mode/recovery matrix.
