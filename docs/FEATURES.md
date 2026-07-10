# Feature inventory

This is the implementation-backed capability ledger as of June 14, 2026. Keep
it updated when behavior ships, changes mode, or gains a known limitation.

Status terms:

- **Available**: implemented in the named mode.
- **Conditional**: implemented when permissions, configuration, or provider
  support are present.
- **Limited**: implemented with a material boundary called out below.
- **Planned**: direction only; not current product behavior.

## Mode matrix

| Capability | Preview | MCP | Local `serve` | Hosted |
|---|---:|---:|---:|---:|
| Read local uncommitted markdown | Available | Available | No | No |
| Graph web UI | Available | No | Available | Available |
| Agent search/traversal tools | No | Available | No | No |
| Search and repository filters | Available | Available | Available | Available |
| Create/edit/delete/rename in UI | No | No | Available | Available |
| Direct GitHub commits | No | No | Conditional | Conditional |
| Pull-request write fallback | No | No | Conditional | Conditional |
| OAuth multi-user login | No | No | No | Available |
| Multi-target routing | No | No | Conditional | Available |
| Webhook/SSE live sync | No | No | Conditional | Conditional |
| Persistent sessions/audit/projection | Projection only, in memory | Projection only, in memory | Available | Available |

## CLI and onboarding

| Capability | Status | Notes |
|---|---|---|
| `gitnodes init [dir]` | Available | Scaffolds config, notes, `AGENTS.md`, agent config for Claude Code/Cursor/Codex/Antigravity (MCP wiring), ignore rules, and best-effort `git init`. Refuses to overwrite scaffold paths. |
| `gitnodes agents [dir]` | Available | Regenerates brain-specific agent instructions from `.gitnodes.yml`. |
| `gitnodes preview [dir]` | Available | Read-only working-tree UI, in-memory projection, loopback-only by default. |
| `gitnodes mcp [dir]` | Available | Read-only stdio server with fingerprint-based projection refresh. |
| `gitnodes doctor [dir] [--json]` | Available | Validates brain structure and reports local/GitHub transition readiness. |
| `gitnodes serve [dir]` | Available | Discovers GitHub target/branch and reuses `gh auth` when explicit auth is absent. |
| Prebuilt installers | Planned | Scripts exist; usable after the public upstream publishes releases. |
| Homebrew/WinGet packages | Planned | Metadata generation exists; external publication is not automated. |

## Knowledge and graph UI

| Capability | Status | Notes |
|---|---|---|
| Markdown/YAML node projection | Available | Git remains authoritative; SQLite is rebuildable. |
| Config-driven node types | Available | Directory, color, labels, title/date/body fields, templates, seed fields, and creatability. |
| Force-directed graph | Available | Branded faceted document nodes and tag hubs; drag, select, hover, zoom, and pan. |
| Edge types | Available | Body links, configured frontmatter links, and shared tags, with legend toggles. |
| Filters | Available | Node type, tag, path, orphan state, and search; filters persist in the URL. |
| Saved views | Available | Named type/tag filter sets with ordering weights. |
| Detail panel | Available | Sanitized markdown, grouped backlinks, related links, metadata, and cover image modal. |
| Mermaid | Available | Lazy-loaded rendering for Mermaid code blocks. |
| Brain switcher | Conditional | Shows confirmed, accessible targets in multi-target deployments. |
| Large graphs | Limited | Current comfort zone is roughly 500 visible nodes; filtering is the mitigation. |

## Search and agent access

| Capability | Status | Notes |
|---|---|---|
| Full-text search | Available | SQLite FTS5 plus structured overlap, combined with reciprocal-rank fusion. |
| Search filters | Available | Type, tag, and path in UI/server/MCP paths. |
| `search_brain` | Available | Ranked local MCP search. |
| `list_nodes` | Available | Enumerates/filter nodes. |
| `read_node` | Available | Returns projected metadata and markdown body. |
| `node_links` | Available | Traverses incoming/outgoing typed graph edges. |
| `validate_brain` | Available | Read-only structural validation for agents. |
| Agent writes through MCP | Not available | By design; agents edit files and use Git. |

## Editing and repository operations

| Capability | Status | Notes |
|---|---|---|
| Create/update notes | Available | Local draft autosave, live preview, templates, related links, and custom string frontmatter fields. |
| Explicit propose-via-PR | Available | Push-capable users may still choose a PR for note saves. |
| Delete notes | Available | SHA/precondition-aware Git mutation. |
| Rename/move notes | Available | Atomic multi-file transaction with markdown-link rewrites. |
| Repository tree/folder browser | Available | Includes structure and orphan discovery. |
| Image upload | Limited | PNG/JPEG/GIF/WebP, maximum 2 MiB, stored under `assets/YYYY/MM/`; requires direct write permission and rejects SVG. |
| Permission-aware writes | Available | Direct commit with `push`; supported mutations fall back to a temporary branch and PR otherwise. |
| Concurrent edit merge UI | Not available | Stale SHA conflicts require reload/reapply or GitHub PR resolution. |
| Frontmatter formatting preservation | Limited | Values survive, but comments, key order, and quoting style do not. |

## Pull requests and review

| Capability | Status | Notes |
|---|---|---|
| Automatic PR fallback | Conditional | Used by supported mutations when direct push is unavailable/protected. |
| Open PR list | Available | Target-scoped list in the web UI. |
| Merge from UI | Limited | Requires write permission and uses squash merge only. |
| Check/mergeability display | Not available | Review status and CI details remain on GitHub. |
| In-app conflict resolution | Not available | GitHub remains the merge surface. |

## Work items

| Capability | Status | Notes |
|---|---|---|
| Work-item kinds | Available | Task, discussion, decision, incident, change, and quote. |
| States and assignees | Available | Projected from frontmatter; editable in the UI. |
| Systems of record | Available | Brain, external, and split. |
| External binding model | Available | Represents GitHub, GitLab, Gitea, Forgejo, and custom providers. |
| GitHub binding and comments | Available | Manual binding to existing issues/items; comments are read from GitHub. |
| Bidirectional GitHub sync | Conditional | State/assignee/label behavior follows system-of-record and label taxonomy. |
| Retry outbox | Available | Background reconciliation, batches of 25, stops after 20 failed attempts for operator review. |
| Non-GitHub provider sync | Planned | Domain representation exists; runtime adapters do not. |
| Automatic issue discovery/binding | Not available | Binding is explicit. |

## Administration and operations

| Capability | Status | Notes |
|---|---|---|
| Config status/error UI | Available | Per-target loading with 30-second cache and legacy filename fallback. |
| Views administration | Available | Preview before/after YAML, stale-SHA protection, direct/PR save. |
| Projection status | Available | Admin visibility into target freshness/materialization. |
| Audit log | Available | Filterable; defaults to 90-day retention. |
| Session list/revocation | Available | Backed by encrypted persistent sessions. |
| Pending provider sync view | Available | Failed/stuck outbox rows remain visible. |
| Projection rebuild | Available | Explicit refresh and webhook-triggered rebuild; no dual-write. |
| Health probes | Available | `/healthz` and `/readyz`. |
| Release-mode embedded assets | Available | Single-file binary extracts versioned assets to a cache directory. |

## Security and access

| Capability | Status | Notes |
|---|---|---|
| Live repository authorization | Available | Pull/push/admin permissions, cached for 15 seconds. |
| OAuth | Available | Optional login-org gate; broad OAuth `repo` scope is a known platform limitation. |
| PAT/GitHub CLI mode | Available | Single-user; remote exposure requires explicit opt-in. |
| Session encryption | Available | Persistent generated key or externally supplied base64 key. |
| CSRF protection | Available | Origin checks on mutating API requests. |
| Rate limiting | Available | Per-IP baseline with proxy-header support. |
| Webhook verification | Available | HMAC-SHA256; unsigned hooks disabled in release unless explicitly allowed. |
| Markdown safety | Available | Raw HTML escaped and output sanitized. |
| Private asset proxy | Available | Fetches repository images server-side without leaking tokens. |

## Sync, storage, and routing

| Capability | Status | Notes |
|---|---|---|
| Target-scoped SQLite projection | Available | Nodes, edges, files, backlinks, work items, FTS, and blob-SHA drift data. |
| Git transaction layer | Available | Atomic multi-file Git Data API commits, preconditions, retry, and PR-branch cleanup. |
| No dual-write | Enforced | Mutations write GitHub; projection changes only through rebuild/sync. |
| GitHub push webhook | Conditional | Rebuilds in the background when webhook and server credential are configured. |
| SSE freshness events | Available | Target-scoped fresh/stale/sync-failed updates. |
| Canonical target routes | Available | `/{owner}/{repo}/{branch}/...`; legacy branchless routes resolve through the target registry. |
| Forge support beyond GitHub | Planned | Routing/domain boundaries anticipate it, but storage/auth are GitHub-specific today. |
