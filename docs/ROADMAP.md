# Brain_UI — Roadmap & Known Caveats

Tracking consolidation work and deferred items. Living document.

## In flight — Consolidation (2026-04)

Three-phase cleanup before adding new features. See plan in repo/PR history.

- **Phase 1 — Scaffolding & safety net** (current)
  - [x] CI workflow (`fmt`, `clippy` ssr+hydrate, `test`, Tailwind build)
  - [x] `justfile` for dev/lint/test/build/docker
  - [x] Structured logging via `tracing` (SSR)
  - [x] This roadmap
- **Phase 2 — Workspace & module boundaries** (done)
  - [x] `brain-domain` — pure types, `BrainError`, frontmatter split (9 tests)
  - [x] `brain-graph` — parsing, graph build, force-directed layout (13 tests)
  - [x] `brain-storage` — reqwest client, `contents_url`, TTL-cached `load_graph` / `load_template`, `invalidate`. All 5 GitHub API calls in `api.rs` use bare reqwest (Octocrab fully removed).
  - [x] `brain-auth` — OAuth primitives (state gen, authorize URL, token exchange, user fetch, org check) + session key constants + session getters. Axum handlers stay in `brain_ui/server/auth.rs` as thin glue that emits audit events.
  - [x] `BrainError` wired through server fns via a single `sfe()` adapter at the edge. Internal code returns typed `Result<T, BrainError>`.
  - [x] Unit tests for parsing / graph build / layout (pure crates, 22 total)
  - [x] Octocrab dep removed — all 5 server fns rewritten to bare reqwest. Build-time & binary-size win.
  - [x] **`Storage` trait extracted.** `brain-storage` exposes a trait with `GithubStorage` and `InMemoryStorage` implementations, and `api.rs` no longer does raw reqwest calls.
  - [x] **`brain-app` extracted.** Moved `src/` under `crates/brain-app/` and retargeted `[package.metadata.leptos]`, Dockerfile builder stage, and `justfile` paths. The workspace root is now purely virtual.
- **Phase 3 — UI consolidation** (done)
  - [x] Shared `<TagBadge>` / `<RemovableBadge>` component in `knowledge/components.rs` — used by `detail_panel.rs`, `detail_bar.rs`, `editor.rs`
  - [x] Accent color via CSS custom properties (`--accent-concept`, etc.) + `NodeType::accent_var()` method (SVG fills still use raw hex)
  - [x] Removed `rel="external"` on internal `/admin` link
  - [x] `graph_version: RwSignal<u64>` replaces `window.location.reload()` — threaded from `KnowledgePage` through `EditorPanel` and `DetailPanel`
  - [x] Decomposed `editor.rs` into `<FrontmatterFields>`, `<TagInput>`, `<RelatedLinksPicker>`, `<MarkdownPreview>` sub-components

## Known caveats

1. **CSRF `state_mismatch` on dropped session cookie** — `/auth/login` stores state in session, `/auth/callback` compares. If the browser drops the cookie between redirects (cross-site cookie policy, incognito) the callback returns `/?error=state_mismatch`. Likely culprit: `SameSite=Lax` vs. GitHub redirect chain. Fix only when it bites.

2. **`SESSION_COOKIE_SECURE` on Railway not verified** — `main.rs` reads the env var; Railway is HTTPS so it should be `1`, but never confirmed in the dashboard. If login starts silently failing in prod, check this env var first.

3. **WASM bundle +80–120 KB from `pulldown-cmark`** — non-optional because the editor renders live preview client-side. If initial load feels slow, revert: make `pulldown-cmark` ssr-only and swap live preview for a debounced `render_markdown_preview` server fn.

4. **`prose-sm` typography sizing is a guess** — tune `tailwind.config.js` `typography.invert` palette and/or swap `prose-sm` → `prose-base` after seeing real content.

5. **~~Editor → reload-on-save~~** — Fixed: `graph_version: RwSignal<u64>` now invalidates the `graph` Resource reactively instead of `window.location.reload()`. No full SSR round-trip on save/delete.

6. **Update path regenerates frontmatter from templates** — if a doc has custom fields (e.g., `status: accepted` on an ADR past-draft), they are wiped on save. Body is preserved verbatim. Fix by round-tripping the parsed frontmatter dict instead of re-emitting from a template during updates.

7. **No auto-refresh after out-of-band commits** — the 30s TTL cache in `runtime.rs` bounds staleness for edits made via `git push` directly. Acceptable; documented here so symptom isn't mistaken for a bug.
