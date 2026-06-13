# gitnodes-app

The GitNodes application: a Leptos 0.8 fullstack web app (SSR binary + WASM
hydrate) with an Axum server. This is the deployable crate.

```mermaid
flowchart TD
    app["gitnodes-app · Leptos + Axum"] --> storage["gitnodes-storage · GitHub + git transactions"]
    app --> auth["gitnodes-auth · OAuth + sessions"]
    storage --> graph["gitnodes-graph · graph build + layout"]
    auth --> domain["gitnodes-domain · pure types"]
    graph --> domain
```

Server-only code is gated behind `feature = "ssr"`, client-only behind
`hydrate`; the binary builds with ssr, the WASM lib with hydrate. Holds the
Axum router (OAuth, webhook, SSE, `/api` server fns, asset proxy), the SQLite
projection, multi-target routing, the permission-aware write orchestrator, and
the Leptos UI.

See the [workspace README](../../README.md) and `CLAUDE.md` for architecture
and invariants.

Licensed under **AGPL-3.0-or-later** (see [LICENSE](LICENSE)); the library
crates it depends on are Apache-2.0.
