# Brain UI

Internal knowledge & edge-administration tool for the Dritara Brain repository.
A Leptos (Rust) fullstack app: reads and renders markdown from a GitHub repo,
writes new documents back via the GitHub API, and visualizes relationships as a graph.
Node types and operational labels are driven by `.brain-config.yml` in the target repo —
no hardcoded types in the binary.

## Stack

- **Rust / Leptos 0.8** — SSR + WASM hydration (`cargo leptos`)
- **Axum 0.8** — HTTP server, session middleware, auth routes
- **tower-sessions 0.14** + `tower-sessions-sqlx-store` — persistent sessions on SQLite
- **reqwest 0.12** — GitHub REST API client (no octocrab)
- **pulldown-cmark** — markdown → HTML, shared between SSR and client (live editor preview)
- **Tailwind CSS 3 + `@tailwindcss/typography`** — styling, built via Node toolchain
- **SQLite** — session store only; content lives in GitHub

## Workspace layout

```
crates/
  brain-domain/    # Pure domain types: BrainConfig, NodeTypeSpec, WorkItem, GithubClient
  brain-graph/     # Graph building + force-directed layout (no I/O)
  brain-storage/   # GitHub API calls: tree walk, file CRUD, asset upload
  brain-auth/      # GitHub OAuth token exchange + org membership check
  brain-app/       # Leptos app + Axum entrypoint (SSR binary + WASM bundle)
    src/
      main.rs                   # Axum entrypoint, session store, auth routes
      api.rs                    # Server functions: read/save/delete/rename brain files
      markdown.rs               # pulldown-cmark wrapper + frontmatter splitter
      server/assets.rs          # Authenticated proxy for private-repo images
      knowledge/
        page.rs                 # /knowledge route composition
        graph_canvas.rs         # SVG graph view
        filter_panel.rs         # Tag + type filters (dynamic from config)
        editor.rs               # Create/update form with live preview
        detail_bar.rs           # Bottom strip: hover/selection summary
        detail_panel.rs         # Right-hand slide-out: rendered markdown + actions
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

## Environment variables

Required at runtime:

| Var                     | Purpose                              |
| ----------------------- | ------------------------------------ |
| `GITHUB_CLIENT_ID`      | GitHub OAuth app client ID           |
| `GITHUB_CLIENT_SECRET`  | GitHub OAuth app client secret       |
| `GITHUB_ORG`            | Target org for membership check      |
| `GITHUB_REPO`           | Target repo (e.g. `Brain`)           |
| `GITHUB_BRANCH`         | Branch to read/write (e.g. `main`)   |

Optional:

| Var                       | Default                | Purpose                                          |
| ------------------------- | ---------------------- | ------------------------------------------------ |
| `SESSION_DB_PATH`         | `data/sessions.db`     | SQLite file for session store                    |
| `LEPTOS_SITE_ADDR`        | `127.0.0.1:3000`       | Bind address                                     |
| `LEPTOS_SITE_ROOT`        | `target/site`          | Static asset root (prod)                         |
| `SESSION_COOKIE_SECURE`   | `0`                    | Set to `1` in HTTPS prod                         |
| `RUST_LOG`                | `brain_ui=info,warn`   | tracing-subscriber env filter                    |

The OAuth app's callback URL must be `{host}/auth/callback`.

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
  -e GITHUB_ORG=Dritara-Digital \
  -e GITHUB_REPO=Brain \
  -e GITHUB_BRANCH=main \
  -v brain_ui_data:/app/data \
  brain_ui
```

Mount `/app/data` on a persistent volume so sessions survive restarts.

## Known caveats & roadmap

See [`docs/ROADMAP.md`](docs/ROADMAP.md) and the `Known caveats` section within.

## License

Internal tool — not published.
