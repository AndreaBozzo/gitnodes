# Operator notes

What GitNodes does today, what it deliberately does not promise, and how to
recover from the known failure modes without reading the code.

## What GitNodes does today

GitNodes is a control plane over a target repository: a Git repository of
markdown files with YAML frontmatter. Git is the single source of truth; SQLite
holds sessions, audit log, target registry, the provider-sync outbox, and a
per-target projection (nodes, edges, files, backlinks, work items) that is always
rebuildable from `git clone` alone.

Shipped and considered stable:

- **Graph + knowledge UI** — typed edges (body links, frontmatter `link_fields`,
  tags), config-driven clusters, saved views, full-text search (FTS5 + RRF),
  markdown rendering with sanitization, Mermaid diagrams (lazy-loaded).
- **Multi-target routing** — canonical `/{owner}/{repo}/{branch}/...` URLs;
  several repositories served by one deployment, each with its own projection,
  config cache, and SSE channel.
- **Editing with permission-aware transactions** — save, delete, rename,
  work-item mutation, config writes and asset uploads use the Git Data API
  transaction layer. Users with `push` commit directly; protected branches or
  limited contributors use automatic PR fallback. Temporary PR branches roll
  back when a commit or PR creation fails.
- **Work items** — markdown documents as tasks with state, assignees, and
  bidirectional GitHub issue/PR binding; provider pushes are best-effort with
  a supervised retry outbox.
- **Inbound sync** — HMAC-verified webhook triggers a background projection
  rebuild; SSE pushes freshness/staleness to connected clients.
- **Org-less operation** — personal repos work end to end; authorization is
  always live `repository_permissions` (pull/push/admin), never org membership.
  `GITHUB_LOGIN_ORG` optionally restricts who may log in.

## What it does not promise

Accepted limitations tracked on the roadmap — do not treat these as bugs:

- **Frontmatter round-trips are lossy.** Saving from the UI reorders keys
  alphabetically and drops YAML comments/quoting style. Users who hand-craft
  frontmatter in an IDE will see noisy diffs.
- **No conflict merge UI.** Concurrent edits surface as a "Stale Data" banner
  and typed conflict errors; resolution is reload-and-reapply or a PR. Richer
  merge tooling is tracked future work.
- **Graph canvas degrades past ~500 visible nodes.** Filters are the
  mitigation; viewport culling is a tracked slice, WebGL is an anti-goal.
- **OAuth tokens carry the broad `repo` scope** — an OAuth App limitation.
  Tokens are encrypted at rest and used server-side only; the GitHub App path
  is the properly scoped remediation.
- **Binary assets are committed to the repo** (`assets/YYYY/MM/`). Asset
  upload in a PR branch breaks live preview until merge; external blob storage
  is a tracked future slice.
- **No issue auto-binding:** binding a work item to an existing GitHub issue is
  manual (number/URL).
- **Rendered HTML is a trust boundary** (partial): markdown is sanitized
  (ammonia, raw HTML escaped) and new `.svg` uploads are rejected, but treat
  embeds and external content as out of scope until their slices land.

## Deploy checklist

Minimum viable environment (see `.env.example` for the annotated version):

1. `GITHUB_CLIENT_ID` / `GITHUB_CLIENT_SECRET` — OAuth App credentials.
2. `TARGET_GITHUB_REPOSITORY=owner/repo` — the default repository. Personal or
   org owner both work.
3. Persistent volume mounted at `data/` — holds `sessions.db` and the
   generated `session.key`. Losing the key invalidates sessions (expected),
   losing the volume also drops audit log and target registry; the projection
   is rebuilt on demand.

Optional, verify intent before setting:

- `GITHUB_LOGIN_ORG` — empty means anyone on GitHub can log in (repo
  permissions still gate every read/write). Set it only to restrict login.
- `WEBHOOK_SECRET` — webhooks stay disabled in release builds until set. With
  webhooks enabled you also need server-side credentials: GitHub App
  (`GITHUB_APP_ID`, `GITHUB_APP_INSTALLATION_ID`, `GITHUB_APP_PRIVATE_KEY[_PATH]`,
  recommended, tokens rotate hourly) or a fine-grained `GITHUB_TOKEN` fallback.
  Required App permissions: Contents R/W, Pull requests R/W, Issues R/W,
  Metadata R.
- `SESSION_COOKIE_SECURE=1` in production behind TLS (a missing Secure cookie is
  the usual cause of OAuth `state_missing` failures).
- Rate limiting (`RATE_LIMIT_PER_SECOND`, default 2; `RATE_LIMIT_BURST`,
  default 60) assumes a reverse proxy setting `X-Forwarded-For`.

Health: `/healthz` (liveness) and `/readyz` (SQLite reachable, projection pool,
session store migrated — returns 503 with a per-check body when not ready).

## Failure modes and recovery

The invariant behind all of them: a failed side effect never corrupts state,
because Git is the only write target and the projection is disposable.

| Symptom | Likely cause | Operator action |
|---|---|---|
| "Stale Data" / `SyncFailed` banner | Webhook rebuild failed or no server credentials | Check warn logs; fix `GITHUB_APP_*`/`GITHUB_TOKEN`; manual refresh from the UI or wait for next push |
| Save fails with conflict | File changed under the user (stale sha) | User reloads and reapplies; no server-side cleanup needed |
| Save silently became a PR | User lacks `push` or branch is protected | Expected fallback — review/merge the PR |
| Config preview says the file changed | `.gitnodes.yml` changed after the preview was generated | Cancel, reload the editor, review the new before/after YAML, then confirm again |
| PR creation failed after branch preparation | GitHub rejected or timed out while opening the PR | The runtime attempts to delete the temporary `patch/...` branch; if a cleanup warning remains in logs, delete that branch manually |
| Work item edit saved but issue not updated | Provider push failed | Row sits in `pending_provider_sync`; supervised retry (App auth) reconciles, gives up after 20 attempts — then fix credentials and retouch the item |
| Types/nodes vanished from the graph | `.gitnodes.yml` no longer parses | "Config invalid" banner shows the parse error and file link; fix the YAML, cache TTL is 30s |
| Login loops with `state_missing` in audit | Secure cookie dropped (no TLS / missing `SESSION_COOKIE_SECURE`) | Set the env, confirm TLS termination; `state_mismatch` instead means replay/stale link |
| Images return 403/502 via `/assets/` | Session expired or upstream GitHub unreachable | Re-login; 502 retries on its own — token is never leaked downstream |
| Projection looks wrong / corrupt | Any | Trigger a rebuild (refresh in UI); worst case delete the projection rows or the DB file — everything regenerates from Git |

## Recovery principle

When in doubt: the repository is correct, everything else is a cache. A full
projection rebuild from a clean clone is always a safe reset; sessions and
audit history are the only data that live exclusively in `data/`.
