# Brain UI

Internal knowledge & edge-administration tool for the Dritara Brain repository.
A Leptos (Rust) fullstack app: reads and renders markdown from a GitHub repo,
writes new documents back via the GitHub API, and visualizes relationships as a graph.
Node types and operational labels are driven by `.brain-config.yml` in the target repo —
no hardcoded types in the binary.

Current architecture: Git remains the source of truth; SQLite now backs sessions,
audit logs, and a target-scoped local projection for graph/work-item reads.
Phase 3 has moved the app from a single-target editor into a multi-tenant
collaborative workspace with target-aware routing, bidirectional work-item sync,
permission-aware direct-write vs PR flows, saved views, repo-structure
navigation, graph polish, canonical target identity, and UI/sidebar posture.
The current focus is shipping discipline: dogfood the closed collaborative core
with a real limited contributor, fix only real blockers, and choose the next
phase from feedback rather than continuing to grow Phase 3.

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
      server/installation_token.rs # GitHub App JWT → installation token, cached + refreshed
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
| `GITHUB_APP_ID`           | _(unset)_              | GitHub App ID. With `GITHUB_APP_INSTALLATION_ID` and a private key, webhooks authenticate as the App (preferred over PAT). |
| `GITHUB_APP_INSTALLATION_ID` | _(unset)_           | Installation ID from the App's `…/settings/installations/<id>` URL after installing on the target org. |
| `GITHUB_APP_PRIVATE_KEY`  | _(unset)_              | Inline PEM of the App's private key. Newlines may be encoded as `\n` for single-line `.env` values. |
| `GITHUB_APP_PRIVATE_KEY_PATH` | _(unset)_          | Alternative to `GITHUB_APP_PRIVATE_KEY` — path to a `.pem` file on disk. Preferred for k8s-style secret mounts. |
| `GITHUB_TOKEN`            | _(unset)_              | Fine-grained PAT used as a fallback when `GITHUB_APP_*` is unset or the App-token mint fails. Without any credential, inbound pushes are signalled as stale and reconciled on next manual refresh. |

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

Webhook-driven projection rebuilds need a server-side credential — set either the `GITHUB_APP_*` trio (preferred, auto-rotating) or `GITHUB_TOKEN` (PAT fallback). On hosts that store env vars as raw strings (Railway, Fly, k8s Secrets), paste the PEM with real newlines; the `\n` escape is only needed for `.env` files.

## Current status

- **Phase 1 closed** — config-driven node types, frontmatter round-trip, `WorkItem` model, real `.brain-config.yml` dogfooding on the Brain repo.
- **Phase 2A/2B closed** — pooled GitHub HTTP client, target-scoped caches, SQLite projection, webhook + SSE baseline, atomic rename via Git Data API, and work-item projection materialization.
- **Phase 3 core closed / closeout active** — multi-tenant routing, Brain Switcher, bidirectional work-item sync, permission-aware branch/PR orchestration, saved views, rate-limit shielding, graph canvas polish, repo structure, canonical `TargetRef`, and UI/sidebar posture are landed. Phase 3 is now frozen to bugfixes, small polish, operator docs, and true dogfooding blockers.
- **Current gate** — validate the collaborative workflow with a real limited contributor on the Brain repo, using the Pokemon mock as a bounded QA artifact.
- **Next hardening lane** — before embeds/blob/AI or larger product expansion: content trust/security baseline, operational failure-mode matrix, and projection schema v2.

## Known caveats & roadmap

See [`docs/ROADMAP.md`](docs/ROADMAP.md) for the detailed roadmap and caveats. As of 2026-05-02, Phase 3 closeout is explicitly framed around:

- a limited contributor using Brain UI against the Brain repo
- branch/PR contribution instead of direct writes to protected `main`
- a Pokemon-themed mock knowledge base with its own `.brain-config.yml`, custom node types/templates, links, assets, work items, saved views, graph controls, and sync
- a short release/operator checklist that says what Brain UI does today, what it does not promise yet, and how to recover from known failure modes

Embedded analytics, BYOB/blob storage, FTS, advisory locks, activity streams,
forge abstraction, temporal graph views, local/offline execution, and richer
conflict resolution remain tracked, but are no longer Phase 3 commitments by
default.

## License

2026@AndreaBozzo -- All rights reserved
