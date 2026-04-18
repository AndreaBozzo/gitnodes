# Brain UI

Internal knowledge & edge-administration tool for the Dritara Brain repository.
A Leptos (Rust) fullstack app: reads and renders markdown from a GitHub repo,
writes new documents back via the GitHub API with enforced frontmatter templates,
and visualizes relationships as a graph.

## Stack

- **Rust / Leptos 0.8** — SSR + WASM hydration (`cargo leptos`)
- **Axum 0.8** — HTTP server, session middleware, auth routes
- **tower-sessions 0.14** + `tower-sessions-sqlx-store` — persistent sessions on SQLite
- **octocrab / reqwest** — GitHub API client
- **pulldown-cmark** — markdown → HTML, shared between SSR (rendering stored files)
  and client (live editor preview)
- **Tailwind CSS 3 + `@tailwindcss/typography`** — styling, built from
  `style/tailwind.css` via Node toolchain (`npm run build:css`)
- **SQLite** — session store only; content lives in GitHub

## Repository layout

```
src/
  main.rs              # Axum entrypoint; SQLite session store; auth routes
  lib.rs               # Module roots
  app.rs               # Leptos Router; public `/` + protected `/knowledge`
  landing.rs           # Unauthenticated landing page
  markdown.rs          # Shared pulldown-cmark wrapper + frontmatter splitter
  api.rs               # Server functions: get_current_user, read/save/delete brain file
  server/auth.rs       # GitHub OAuth (login, callback w/ CSRF state, logout, middleware)
  knowledge/
    page.rs            # Main /knowledge composition
    graph_canvas.rs    # SVG graph view
    filter_panel.rs    # Tag + type filters
    editor.rs          # New-document form with live preview
    detail_bar.rs      # Bottom strip: hover/selection summary
    detail_panel.rs    # Right-hand slide-out: full rendered markdown
    types.rs           # Node, Edge, NodeType, BrainFilePayload
    data.rs            # Generated at build time by build.rs
build.rs               # Parses BRAIN_DIR at compile time into a static graph
style/
  tailwind.css         # Tailwind source (input)
  main.css             # Generated output (gitignored)
tailwind.config.js     # Content scan + prose-invert palette
package.json           # Build-time-only: tailwindcss + typography plugin
```

## Environment variables

Required at runtime (SSR binary):

| Var                   | Purpose                                              |
| --------------------- | ---------------------------------------------------- |
| `GITHUB_CLIENT_ID`     | GitHub OAuth app client ID                          |
| `GITHUB_CLIENT_SECRET` | GitHub OAuth app client secret                      |

Optional:

| Var                 | Default              | Purpose                             |
| ------------------- | -------------------- | ----------------------------------- |
| `SESSION_DB_PATH`   | `data/sessions.db`   | SQLite file for session store       |
| `LEPTOS_SITE_ADDR`  | `127.0.0.1:3000`     | Bind address                        |
| `LEPTOS_SITE_ROOT`  | `target/site`        | Static asset root (prod)            |
| `BRAIN_DIR`         | `../Brain`           | Path to the Brain repo at **build time** |

The OAuth app's callback URL must be `{host}/auth/callback`.

## Local development

Prereqs: Rust toolchain from `rust-toolchain.toml`, Node 18+, `cargo-leptos`, the
`wasm32-unknown-unknown` target.

```bash
npm install                     # once — installs tailwind + typography plugin
npm run watch:css &             # rebuild style/main.css on .rs changes
cargo leptos watch              # rebuild server + hydrate on source changes
```

Put OAuth secrets in `.env` (gitignored). Register the callback URL as
`http://127.0.0.1:3000/auth/callback`.

## Production build

The Dockerfile runs the CSS pipeline in a Node stage, then builds the Rust
fullstack app against the compiled CSS, then ships a minimal `debian:bookworm-slim`
runtime:

```bash
docker build -t brain_ui .
docker run -p 3000:3000 \
  -e GITHUB_CLIENT_ID=... \
  -e GITHUB_CLIENT_SECRET=... \
  -v brain_ui_data:/app/data \
  brain_ui
```

Mount `/app/data` on a persistent volume so session cookies survive restarts.

## Auth flow

1. Unauthenticated visitor hits `/` → sees the landing page with "Login with GitHub"
2. Click → `GET /auth/login` generates a CSRF `state`, stores it in the session,
   redirects to GitHub
3. GitHub returns to `/auth/callback?code=...&state=...` — server verifies state,
   exchanges code for access token, fetches user login, stores both in the session
4. Redirects to `/knowledge`
5. Direct access to `/knowledge*` without a session token is redirected to `/` via
   the `protect_knowledge` middleware in `main.rs`

## Write path

`EditorPanel` gathers structured inputs (type, title, tags, body, related links),
sends `BrainFilePayload` to the `save_brain_file` server function. Server-side,
`generate_markdown` assembles the full file with Brain-template frontmatter, base64-
encodes it, and `PUT`s to the GitHub Contents API under the authenticated user's
token.

**Caveat**: the graph is baked at `build.rs` time from `BRAIN_DIR`. Newly created
documents won't appear in the graph until the app is rebuilt. Runtime merging is
tracked as a Phase-4 decision.

## Known limitations

See [`.claude/projects/.../memory/open_caveats.md`](#) or ask during an e2e session.
Short list: graph doesn't auto-refresh after commits; WASM bundle includes
pulldown-cmark (~100KB); `prose-sm` sizing is untuned; CSRF state depends on session
cookie surviving the GitHub round-trip.

## License

Internal tool — not published.
