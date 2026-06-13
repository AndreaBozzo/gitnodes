# AGENTS.md

This file provides guidance to coding agents (Claude Code, Codex, Cursor, …) and human contributors working in this repository. It is the canonical contributor guide; `CLAUDE.md` imports it.

## What this is

GitNodes is a Leptos 0.8 fullstack (SSR + WASM hydrate) web app over a "Brain": a GitHub repo of markdown files with YAML frontmatter, visualized as a graph and editable in-app. Git is the single source of truth; SQLite is a rebuildable, target-scoped projection (never a primary store). `docs/ROADMAP.md` holds the public, high-level direction.

## Commands

Dev workflow uses `just` (see `justfile`):

```bash
just setup        # npm ci (Tailwind pipeline, one-off)
just css-watch    # Tailwind watch (separate terminal, required for dev)
just dev          # cargo leptos watch -p gitnodes-app
just build        # release build: SSR binary + hydrate WASM
just check        # fmt + lint + test, same set CI runs
```

The CI gates, runnable individually:

```bash
cargo fmt --all -- --check
cargo clippy -p gitnodes-app --no-default-features --features ssr -- -D warnings
cargo clippy -p gitnodes-app --no-default-features --features hydrate --target wasm32-unknown-unknown -- -D warnings
cargo clippy --workspace --exclude gitnodes-app -- -D warnings
cargo test -p gitnodes-app --no-default-features --features ssr
cargo test --workspace --exclude gitnodes-app
```

Single test: `cargo test -p gitnodes-app --no-default-features --features ssr <test_name>` (gitnodes-app tests always need the feature flags; other crates don't). Tests live inline in `src/` under `#[cfg(test)]`, not in `tests/` directories. GitHub API tests use `wiremock`; projection tests use in-memory SQLite.

Local run needs `GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET`, `TARGET_GITHUB_REPOSITORY=owner/repo` (see `.env.example` for the rest).

## Architecture

Five crates, strict dependency direction (app → storage/auth → graph → domain):

- `gitnodes-domain` — pure types (BrainConfig, Node, Edge, WorkItem, frontmatter split). No I/O, WASM-safe.
- `gitnodes-graph` — graph build from markdown (typed edges: Body/Frontmatter/Tag), link resolution, force-directed layout. Pure logic, WASM-safe.
- `gitnodes-auth` — GitHub OAuth primitives, org membership, session token storage.
- `gitnodes-storage` — GitHub API client; `git_transaction.rs` is the atomic multi-file commit layer (Git Data API, preconditions, retry with jitter) and the most mature piece of the workspace — Phase 4 builds on it.
- `gitnodes-app` — Leptos app + Axum server. Server-only code is gated behind `feature = "ssr"`; client-only behind `hydrate`. The binary builds with ssr, the WASM lib with hydrate. Anything using tokio/reqwest/sqlx must be `#[cfg(feature = "ssr")]`.

Key gitnodes-app areas:

- `src/main.rs` — subcommand dispatch (`serve`/`init`/`agents`/`mcp`) then the Axum router: OAuth routes, `/webhook/github` (HMAC-verified, fires background sync), `/sse/events`, `/api/{fn}` server functions, asset proxy with locked-down CSP, CSRF origin check on mutating POSTs.
- `src/cli.rs` — the `gitnodes` subcommands for first-run setup: `init` scaffolds a starter brain (embedded from `examples/starter-brain/`) + `AGENTS.md`, `agents` (re)generates `AGENTS.md` from `.gitnodes.yml`, `serve` discovers the target from the Git remote and reuses `gh auth`.
- `src/mcp.rs` — read-only local MCP server (`gitnodes mcp [dir]`, stdio) exposing `search_brain`/`list_nodes`/`read_node`/`node_links` over an in-memory SQLite projection of the working tree; `refresh()` debounces via a size+mtime fingerprint so repeated tool calls don't rebuild.
- `src/server/pat.rs` — single-user PAT mode (`GITHUB_PAT`): the PAT is the token for every call and the session is the PAT owner; refuses a non-loopback bind unless `GITNODES_ALLOW_REMOTE_PAT` is set. Authorization still flows through live `repository_permissions`.
- `src/server/embedded.rs` — `embed-assets` feature compiles `target/site` into the binary for single-file distribution; extracted once to a per-version cache dir at startup.
- `src/server/projection/` — SQLite projection (nodes, edges, files, backlinks, work_items + blob_sha drift detection). Rebuild is explicit: fetch raw files → build graph → persist snapshot → watermark in `projection_sync_state`.
- `src/server/routing.rs` — multi-target routing via `TargetRef`: 4-segment canonical `/{org}/{repo}/{branch}/...`, 3-segment legacy resolved through the `target_registry` table.
- `src/api/write_orchestrator.rs` — permission-aware writes: direct commit if `push`, PR fallback otherwise; `WriteIntent::ProposeViaPr` lets users with push rights opt into PR flow.
- `src/server/access.rs` — authorization is per-request `repository_permissions` (pull/push/admin) with a 15s cache; works org-less on personal repos. `GITHUB_LOGIN_ORG` optionally gates login.
- Server credentials for webhooks: GitHub App installation token first (`installation_token.rs`), `GITHUB_TOKEN` PAT fallback.

## Invariants and gotchas

- No Dual-Write: mutations go to GitHub via `GitTransaction`/the orchestrator; the projection only updates through rebuild/webhook, never directly alongside a write.
- The projection must stay rebuildable from `git clone` alone; derived indexes (FTS5 etc.) are artifacts, never sources of truth.
- Only persist `target_registry` rows for confirmed Brains, never for bare repo listings.
- `#![recursion_limit = "512"]` in both `main.rs` and `lib.rs` — Leptos macros need it; exceeding it gives cryptic errors.
- Tailwind classes are scanned from `.rs` files (`tailwind.config.js`); CSS is a separate Node pipeline, not cargo.
- Every `<a>` pointing outside the Leptos router needs `rel="external"` or the client router swallows the navigation.
- Frontmatter round-trip is lossy (key reorder, comments dropped) — known debt, tracked in ROADMAP; don't "fix" it ad hoc in `merge_frontmatter`.
- `.gitnodes.yml` is fetched per-target with a 30s TTL cache; missing config falls back to `BrainConfig::default()`.
- Server fn auto-registration has a guard test against LTO stripping in release — keep it passing.
