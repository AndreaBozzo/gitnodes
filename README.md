# Brain UI

Internal knowledge & edge-administration tool for the Dritara Brain repository.
A Leptos (Rust) fullstack app: reads and renders markdown from a GitHub repo,
writes new documents back via the GitHub API, and visualizes relationships as a graph.
Node types and operational labels are driven by `.brain-config.yml` in the target repo —
no hardcoded types in the binary.

Current architecture: Git remains the source of truth; SQLite now backs sessions,
audit logs, and a target-scoped local projection for graph/work-item reads.
The current Phase 3 objective is evolving the app from a single-target editor into
a multi-tenant collaborative workspace with target-aware routing, bidirectional
work-item sync, and permission-aware direct-write vs PR flows.

## Stack

- **Rust / Leptos 0.8** — SSR + WASM hydration (`cargo leptos`)
- **Axum 0.8** — HTTP server, session middleware, auth routes
- **tower-sessions 0.14** + `tower-sessions-sqlx-store 0.15` — persistent sessions on SQLite
- **reqwest 0.12** — GitHub REST API client (no octocrab)
- **pulldown-cmark** — markdown → HTML, shared between SSR and client (live editor preview)
- **Tailwind CSS 3 + `@tailwindcss/typography`** — styling, built via Node toolchain
- **SQLite** — sessions, audit log, and target-scoped projection (`nodes`, `edges`, `files`, `backlinks`, `work_items`, `work_item_bindings`); content source of truth still lives in GitHub

## Workspace layout

```
crates/
  brain-domain/    # Pure domain types: BrainConfig, NodeTypeSpec, WorkItem, GithubClient
  brain-graph/     # Graph building + force-directed layout (no I/O)
  brain-storage/   # GitHub API calls: tree walk, file CRUD, asset upload, atomic Git Data commits
  brain-auth/      # GitHub OAuth token exchange + org membership check
  brain-app/       # Leptos app + Axum entrypoint (SSR binary + WASM bundle)
    src/
      main.rs                   # Axum entrypoint, session store, auth routes
      api.rs                    # Server functions: graph/file/work-item reads, writes, rebuilds
      markdown.rs               # pulldown-cmark wrapper + frontmatter splitter
      server/assets.rs          # Authenticated proxy for private-repo images
      server/projection.rs      # SQLite projection materialization + read model
      server/webhook.rs         # GitHub webhook entrypoint (push baseline)
      server/sse.rs             # Typed SSE event bus + stream endpoint
      knowledge/
        page.rs                 # /knowledge route composition
        graph_canvas.rs         # SVG graph view
        filter_panel.rs         # Tag + type filters (dynamic from config)
        editor.rs               # Create/update form with live preview
        detail_bar.rs           # Bottom strip: hover/selection summary
        detail_panel.rs         # Right-hand slide-out: rendered markdown + work-item card
        orphan_banner.rs        # Amber advisory for unknown node types
        config_loader.rs        # 30s TTL cache for .brain-config.yml
        draft.rs                # localStorage autosave (schema v2)
docs/
  ROADMAP.md
```

## Configuration

Node types are declared in `.brain-config.yml` at the root of the target repo.
The binary ships a built-in default equivalent to the seven pre-Phase-1 types
(concept, adr, meeting, post-mortem, preventivo, runbook, tag), so repos without
the file keep working unchanged.

See the [Brain repo config](https://github.com/Dritara-Digital/Brain/blob/main/.brain-config.yml)
for a real-world example.

Work items are configured the same way: node types can declare `work_item_kind`, and
the label taxonomy in `.brain-config.yml` drives provider-facing labels without hardcoding
GitHub-specific names in the app.

## Environment variables

Required at runtime:

| Var                     | Purpose                              |
| ----------------------- | ------------------------------------ |
| `GITHUB_CLIENT_ID`      | GitHub OAuth app client ID           |
| `GITHUB_CLIENT_SECRET`  | GitHub OAuth app client secret       |
| `TARGET_GITHUB_ORG`     | Target org for login gating and repo access |
| `TARGET_GITHUB_REPO`    | Target repo (e.g. `Brain`)           |
| `TARGET_GITHUB_BRANCH`  | Branch to read/write (e.g. `main`)   |

Optional:

| Var                       | Default                | Purpose                                          |
| ------------------------- | ---------------------- | ------------------------------------------------ |
| `SESSION_DB_URL`          | `sqlite://data/sessions.db` | SQLite database URL for sessions, audit log, and local projection |
| `LEPTOS_SITE_ADDR`        | `127.0.0.1:3000`       | Bind address                                     |
| `LEPTOS_SITE_ROOT`        | `target/site`          | Static asset root (prod)                         |
| `SESSION_COOKIE_SECURE`   | `0`                    | Set to `1` in HTTPS prod                         |
| `RUST_LOG`                | `brain_ui=info,warn`   | tracing-subscriber env filter                    |
| `WEBHOOK_SECRET`          | _(unset)_              | HMAC-SHA256 secret matching the GitHub webhook config; if unset the `/webhook/github` endpoint accepts unsigned payloads (dev only) |
| `GITHUB_TOKEN`            | _(unset)_              | Server-side token used by the webhook handler to rebuild the projection on `push`. Without it inbound pushes are signalled as stale and reconciled on next manual refresh |

Branding is also required at runtime:

| Var                       | Purpose                                          |
| ------------------------- | ------------------------------------------------ |
| `BRAND_NAME`              | UI brand shown in the header and page title      |
| `BRAND_ORG_LABEL`         | Org label used in access-denied copy             |

The OAuth app's callback URL must be `{host}/auth/callback`.

Legacy aliases `GITHUB_ORG`, `GITHUB_REPO`, and `GITHUB_BRANCH` are still accepted at runtime for backward compatibility, but new deploys should use the explicit `TARGET_GITHUB_*` names.

## Local development

Prereqs: Rust toolchain from `rust-toolchain.toml`, Node 18+, `cargo-leptos`,
`wasm32-unknown-unknown` target, optionally [`just`](https://github.com/casey/just).

```bash
just setup        # once — installs tailwind + typography plugin
just css-watch &  # rebuild style/main.css on changes
just dev          # cargo leptos watch
```

Or without `just`: `npm install`, `npm run watch:css &`, `cargo leptos watch`.

Put OAuth secrets and target repo vars in `.env` (gitignored).

## Production build

```bash
docker build -t brain_ui .
docker run -p 3000:3000 \
  -e GITHUB_CLIENT_ID=... \
  -e GITHUB_CLIENT_SECRET=... \
  -e TARGET_GITHUB_ORG=Dritara-Digital \
  -e TARGET_GITHUB_REPO=Brain \
  -e TARGET_GITHUB_BRANCH=main \
  -e BRAND_NAME="Dritara Brain" \
  -e BRAND_ORG_LABEL=Dritara-Digital \
  -v brain_ui_data:/app/data \
  brain_ui
```

Mount `/app/data` on a persistent volume so sessions survive restarts.

## Current status

- **Phase 1 closed** — config-driven node types, frontmatter round-trip, `WorkItem` model, real `.brain-config.yml` dogfooding on the Brain repo.
- **Phase 2A/2B closed** — pooled GitHub HTTP client, target-scoped caches, SQLite projection, webhook + SSE baseline, atomic rename via Git Data API, and work-item projection materialization.
- **Current gap** — work items are already accessible in the UI as local operational fields and read models, but forge-bound mutations, multi-target routing, and permission-aware branch/PR orchestration are the next step.

## Known caveats & roadmap

See [`docs/ROADMAP.md`](docs/ROADMAP.md) for the detailed roadmap and caveats. As of 2026-04-26, Phase 3 is explicitly framed around:

- target-aware routing `/{org}/{repo}` and a Brain Switcher
- bidirectional work-item sync via the user's OAuth token
- RBAC/capability-aware save orchestration with direct-write vs branch+PR fallback

Phase 4 then standardizes the forge boundary (`ForgeAdapter`), temporal graph views, local/offline execution, and richer conflict resolution.

## License

2026@AndreaBozzo -- All rights reserved
