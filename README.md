# GitNodes

GitNodes turns a GitHub repository of markdown files into a navigable, editable
knowledge graph. It reads and renders markdown with YAML frontmatter, writes new
and edited documents back through the GitHub API, and visualizes the
relationships between them as a force-directed graph. Node types and operational
labels are driven by a `.gitnodes.yml` file in the target repo — there are no
hardcoded types in the binary.

Git stays the single source of truth: SQLite is only a rebuildable,
target-scoped projection that backs sessions, audit logs, and graph/work-item
reads. The app is a Leptos (Rust) fullstack application — server-rendered with
WASM hydration — supporting multi-repository routing, bidirectional work-item
sync, permission-aware direct-write vs pull-request flows, saved views, and
repo-structure navigation.

## Quickstart

Install the prebuilt binary — no Rust toolchain or compiling:

```bash
# macOS / Linux
curl -fSLo install-gitnodes.sh https://raw.githubusercontent.com/AndreaBozzo/gitnodes/master/install.sh
less install-gitnodes.sh
sh install-gitnodes.sh
```

```powershell
# Windows (PowerShell)
Invoke-WebRequest https://raw.githubusercontent.com/AndreaBozzo/gitnodes/master/install.ps1 -OutFile install-gitnodes.ps1
Get-Content .\install-gitnodes.ps1
& .\install-gitnodes.ps1
```

Then scaffold a knowledge base and run it:

```bash
gitnodes init my-brain      # starter notes + .gitnodes.yml + AGENTS.md, git-initialised
cd my-brain
git add . && git commit -m "Initialize GitNodes knowledge base"
gh repo create my-brain --private --source=. --remote=origin --push
gitnodes serve              # discovers the repo, reuses `gh auth`, opens the browser
```

If needed, run `gh auth login` once before the commands above. GitNodes reads the
repository and branch from the local Git checkout and uses the credential already
stored by GitHub CLI; it does not copy that token into `.env` or another file.
`GITHUB_PAT` remains available as an explicit single-user fallback.
The scaffolded `AGENTS.md` teaches coding agents (Claude Code, Codex, Cursor, …)
the conventions of your brain so they can add and link notes correctly. GitNodes
is built for humans and agents alike.

## AI agent access

GitNodes includes a read-only local MCP server. Point any stdio-capable MCP
client at the repository:

```json
{
  "mcpServers": {
    "gitnodes": {
      "command": "gitnodes",
      "args": ["mcp", "/absolute/path/to/my-brain"]
    }
  }
}
```

The server exposes `search_brain`, `list_nodes`, and `read_node`. It re-indexes
the current working tree before each request through the same SQLite projection
and FTS5 search path used by the web UI, so uncommitted notes are visible
immediately. It is local and read-only: no PAT, GitHub login, push, or running
web server is required.

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
  gitnodes-domain/    # Pure domain types: BrainConfig, NodeTypeSpec, WorkItem, GithubClient
  gitnodes-graph/     # Graph building + force-directed layout (no I/O)
  gitnodes-storage/   # GitHub API calls: tree walk, file CRUD, asset upload, atomic Git Data commits
  gitnodes-auth/      # GitHub OAuth token exchange + optional org membership check
  gitnodes-app/       # Leptos app + Axum entrypoint (SSR binary + WASM bundle)
    src/
      main.rs                   # Axum entrypoint, session store, auth routes
      api.rs                    # Server functions: graph/file/work-item reads, writes, rebuilds
      mcp.rs                    # Read-only local agent tools over stdio MCP
      markdown.rs               # pulldown-cmark wrapper + frontmatter splitter
      server/assets.rs          # Authenticated proxy for private-repo images
      server/projection/        # SQLite projection materialization + read model
      server/health.rs          # /healthz and /readyz operational probes
      server/pending_sync_job.rs # Background retry loop for provider sync outbox
      server/webhook.rs         # GitHub webhook entrypoint (push + item sync)
      server/sse.rs             # Per-target typed SSE event bus + stream endpoint
      server/installation_token.rs # GitHub App JWT → installation token, cached + refreshed
      knowledge/
        page.rs                 # /knowledge route composition
        graph_canvas.rs         # SVG graph view
        filter_panel.rs         # Tag + type filters (dynamic from config)
        editor/                 # Create/update form split into focused submodules
        detail_bar.rs           # Bottom strip: hover/selection summary
        detail_panel.rs         # Right-hand slide-out: rendered markdown + work-item card
        orphan_banner.rs        # Amber advisory for unknown node types
        config_loader.rs        # 30s TTL cache for .gitnodes.yml
        draft.rs                # localStorage autosave (schema v2)
docs/
  ROADMAP.md
```

## Configuration

Node types are declared in `.gitnodes.yml` at the root of the target repo.
The binary ships a built-in default equivalent to seven starter types
(concept, adr, meeting, post-mortem, project, runbook, tag), so repos without
the file keep working unchanged. Repos created before the rename are still read
from a legacy `.brain-config.yml` if `.gitnodes.yml` is absent.

The built-in default doubles as a worked example: any repo of markdown files
with YAML frontmatter works as a target, with or without a config file.

Work items are configured the same way: node types can declare `work_item_kind`, and
the label taxonomy in `.gitnodes.yml` drives provider-facing labels without hardcoding
GitHub-specific names in the app.

Saved views accept an optional `weight:` (integer; lower = earlier, default 0) so a
single pinned view can float to the top without re-ordering every entry. Individual
notes can declare an optional `cover:` in their frontmatter — a repo-relative image
path or an absolute `https://` URL — to render a hero image at the top of the detail
panel. Backlinks in the detail panel are grouped by node type, in the same order as
`node_types[]`.

### Typed graph edges (`link_fields`)

Node types can opt into **typed edges** by declaring `link_fields:` — a map from a
frontmatter field name to the target node type. The graph builder resolves slug
values in those fields against existing files and materializes edges tagged with
the source field name, alongside the body-link edges that already exist.

```yaml
- name: pokemon
  directory: pokemon
  link_fields:
    trainer: trainer          # pokemon.trainer  → ownership
    locations: route          # pokemon.locations → encounter geography
    evolves_to: pokemon       # pokemon.evolves_to → evolution chain
```

The canvas styles edges by their kind (`Body`, `Frontmatter(field)`, `Tag`) and
exposes a toggle legend in the bottom-left so users can isolate ownership,
geography, evolution, or tag relations from narrative body citations. Slugs that
don't resolve to an existing file are silently ignored — useful for documenting
future entities without breaking the graph. The field is optional and
backward-compatible (empty = no typed edges).

## Environment variables

For local `gitnodes serve [dir]`, the target repository, branch, and credential
are discovered from the Git checkout and GitHub CLI login. Explicit environment
configuration always takes precedence.

Required for deployments or checkouts without an `origin` remote:

| Var                        | Purpose                                   |
| -------------------------- | ----------------------------------------- |
| `TARGET_GITHUB_REPOSITORY` | Default repository in `owner/repo` format |

Choose one authentication mode:

| Var                    | Purpose |
| ---------------------- | ------- |
| `GITHUB_PAT`           | Single-user mode: use this PAT for every GitHub request; no OAuth App required. |
| `GITHUB_CLIENT_ID` + `GITHUB_CLIENT_SECRET` | Multi-user mode: GitHub OAuth App credentials. |

Optional:

| Var                       | Default                | Purpose                                          |
| ------------------------- | ---------------------- | ------------------------------------------------ |
| `TARGET_GITHUB_BRANCH`    | `main`                 | Branch to read/write. |
| `GITNODES_ALLOW_REMOTE_PAT` | _(unset)_            | Set to `1` only when deliberately exposing PAT mode beyond loopback behind your own access control. |
| `GITNODES_NO_OPEN`        | _(unset)_              | Disable automatically opening the browser on a loopback bind. |
| `GITHUB_LOGIN_ORG`        | _(org-less)_           | Optional organization required at login. Target access remains gated by live repository permissions. |
| `BRAND_NAME`              | `GitNodes`             | UI brand shown in the header and page title. |
| `BRAND_ORG_LABEL`         | repository owner       | Owner label used in access-denied copy. |
| `SESSION_DB_URL`          | `sqlite://data/sessions.db` | SQLite database URL for sessions, audit log, and local projection |
| `LEPTOS_SITE_ADDR`        | `127.0.0.1:3000`       | Bind address                                     |
| `LEPTOS_SITE_ROOT`        | `target/site`          | Static asset root (prod)                         |
| `SESSION_COOKIE_SECURE`   | `1` in release, `0` in debug | Marks the session cookie Secure. Override to `0` only for local HTTP dev. |
| `SESSION_ENCRYPTION_KEY_FILE` | `data/session.key` | Persistent generated key file used when `SESSION_ENCRYPTION_KEY` is unset. |
| `SESSION_ENCRYPTION_KEY`  | _(generated in key file)_ | Explicit base64 key (>=64 bytes decoded), useful for external secret management. |
| `RUST_LOG`                | `gitnodes_app=info,warn` | tracing-subscriber env filter                  |
| `WEBHOOK_SECRET`          | _(webhook disabled)_   | HMAC-SHA256 secret matching the GitHub webhook config. Setting it enables the endpoint. |
| `ALLOW_INSECURE_WEBHOOKS` | `1` in debug, `0` in release | Explicitly allows unsigned `/webhook/github` requests. Dev-only escape hatch. |
| `RATE_LIMIT_PER_SECOND`   | `2`                    | Per-IP request rate for the baseline governor.   |
| `RATE_LIMIT_BURST`        | `60`                   | Per-IP burst capacity for the baseline governor. |
| `GITHUB_APP_ID`           | _(unset)_              | GitHub App ID. With `GITHUB_APP_INSTALLATION_ID` and a private key, webhooks authenticate as the App (preferred over PAT). |
| `GITHUB_APP_INSTALLATION_ID` | _(unset)_           | Installation ID from the App's `…/settings/installations/<id>` URL after installing on the target org. |
| `GITHUB_APP_PRIVATE_KEY`  | _(unset)_              | Inline PEM of the App's private key. Newlines may be encoded as `\n` for single-line `.env` values. |
| `GITHUB_APP_PRIVATE_KEY_PATH` | _(unset)_          | Alternative to `GITHUB_APP_PRIVATE_KEY` — path to a `.pem` file on disk. Preferred for k8s-style secret mounts. |
| `GITHUB_API_BASE`         | `https://api.github.com` | GitHub REST API base for App-token minting and GHES-style test/deploy targets. |
| `GITHUB_TOKEN`            | _(unset)_              | Fine-grained PAT used as a fallback when `GITHUB_APP_*` is unset or the App-token mint fails. Without any credential, inbound pushes are signalled as stale and reconciled on next manual refresh. |
| `PENDING_SYNC_INTERVAL_SECS` | `60`                | Poll interval for the provider-sync outbox retry job. |

The OAuth app's callback URL must be `{host}/auth/callback`.

Existing deployments may keep `TARGET_GITHUB_ORG`, `TARGET_GITHUB_REPO`, and
their legacy `GITHUB_*` aliases. Those split variables retain the historical
login organization fallback. New deployments using
`TARGET_GITHUB_REPOSITORY` default to org-less login.

Any GitHub user can complete OAuth in the default setup, but GitNodes serves a
target only when GitHub reports live `pull` permission. Write and administration
capabilities continue to follow `push`, `maintain`, and `admin`.

## Local development

Prereqs: Rust toolchain from `rust-toolchain.toml`, Node 18+, `cargo-leptos`,
`wasm32-unknown-unknown` target, optionally [`just`](https://github.com/casey/just).

```bash
just setup        # once — installs tailwind + typography plugin
just css-watch &  # rebuild style/main.css on changes
just dev          # cargo leptos watch
```

Or without `just`: `npm install`, `npm run watch:css &`, `cargo leptos watch`.

Put the three required values in `.env` (gitignored).

## Production build

```bash
docker build -t gitnodes .
docker run -p 3000:3000 \
  -e GITHUB_CLIENT_ID=... \
  -e GITHUB_CLIENT_SECRET=... \
  -e TARGET_GITHUB_REPOSITORY=your-owner/your-repository \
  -v gitnodes_data:/app/data \
  gitnodes
```

Mount `/app/data` on a persistent volume so sessions and the generated
encryption key survive restarts.

Webhook-driven projection rebuilds need a server-side credential — set either the `GITHUB_APP_*` trio (preferred, auto-rotating) or `GITHUB_TOKEN` (PAT fallback). On hosts that store env vars as raw strings (Railway, Fly, k8s Secrets), paste the PEM with real newlines; the `\n` escape is only needed for `.env` files.

## Status & roadmap

GitNodes is built on a mature core: config-driven node types, an atomic
multi-file Git commit layer, a rebuildable SQLite projection, webhook + SSE live
sync, multi-repository routing, bidirectional work-item sync, and
permission-aware direct-write vs pull-request flows are all in place. Security,
operational-readiness (`/healthz`, `/readyz`, rate limiting, session
encryption), and schema-operations hardening lanes are closed.

See [`docs/ROADMAP.md`](docs/ROADMAP.md) for the overall direction.

## License

This workspace is split-licensed:

- The **library crates** — `gitnodes-domain`, `gitnodes-graph`, `gitnodes-auth`,
  `gitnodes-storage` — are licensed under the
  [Apache License, Version 2.0](LICENSE-APACHE). Reuse them freely.
- The **deployable application** — `gitnodes-app` — is licensed under the
  [GNU Affero General Public License v3.0 or later](LICENSE-AGPL). If you run a
  modified GitNodes as a network service, the AGPL requires you to offer your
  users the corresponding source.

`gitnodes-app` incorporates the Apache-2.0 libraries (one-way compatible into the
AGPL), so the combined application is distributed under the AGPL while the
libraries remain independently usable under Apache-2.0.

Copyright (C) 2026 Andrea Bozzo.
