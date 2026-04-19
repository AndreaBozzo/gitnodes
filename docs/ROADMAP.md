# Brain_UI — Roadmap & Known Caveats

Tracking consolidation work and deferred items. Living document.

## In flight — Consolidation (2026-04)

Three-phase cleanup before adding new features. See plan in repo/PR history.

- **Phase 1 — Scaffolding & safety net** (current)
  - [x] CI workflow (`fmt`, `clippy` ssr+hydrate, `test`, Tailwind build)
  - [x] `justfile` for dev/lint/test/build/docker
  - [x] Structured logging via `tracing` (SSR)
  - [x] This roadmap
- **Phase 2 — Workspace & module boundaries** (mostly done)
  - [x] `brain-domain` — pure types, `BrainError`, frontmatter split (9 tests)
  - [x] `brain-graph` — parsing, graph build, force-directed layout (13 tests)
  - [x] `brain-storage` — Octocrab client, `contents_url`, TTL-cached `load_graph` / `load_template`, `invalidate`. Collapses the 5× Octocrab boilerplate previously in `api.rs`.
  - [x] `brain-auth` — OAuth primitives (state gen, authorize URL, token exchange, user fetch, org check) + session key constants + session getters. Axum handlers stay in `brain_ui/server/auth.rs` as thin glue that emits audit events.
  - [x] `BrainError` wired through server fns via a single `sfe()` adapter at the edge. Internal code returns typed `Result<T, BrainError>`.
  - [x] Unit tests for parsing / graph build / layout (pure crates, 22 total)
  - [ ] **Deferred — no `Storage` trait yet.** `brain-storage` exposes concrete functions tied to Octocrab + the Brain repo. A trait (with an in-memory impl for tests) is only worth it once we have a second backend or want to exercise `api.rs` write paths in tests. Revisit when either lands.
  - [ ] **Deferred — `brain-app` extraction.** Moving `src/` under `crates/brain-app/` means retargeting `[package.metadata.leptos]`, the Dockerfile builder stage, and `cargo leptos watch` paths. High churn for no architectural win today since the root package is the sole top-level bin; do it only when we need a second binary (CLI, migration tool) sharing the app crate.
  - [ ] Octocrab dep is used in only 2 of 5 call sites (read_brain_file, list_brain_folders); the other 3 use crab._put/_delete which are raw reqwest wrappers anyway. Swapping those two to bare reqwest drops the entire octocrab dep — meaningful build-time & binary-size win, ~neutral LOC.
- **Phase 3 — UI consolidation** (not started)
  - [ ] Shared `<Badge>` / `<Tag>` component (tag markup duplicated across `editor.rs`, `detail_panel.rs`, `detail_bar.rs`, `filter_panel.rs`)
  - [ ] Accent color via CSS var instead of inline `style=format!("background:{}", t.accent())` (8 sites)
  - [ ] Remove `rel="external"` on internal `/admin` link (`page.rs:98`)
  - [ ] `graph_version: RwSignal<u64>` to replace `window.location.reload()` in `editor.rs:209` and `detail_panel.rs:62`
  - [ ] Decompose `editor.rs` (469 LOC) into `<FrontmatterFields>`, `<TagInput>`, `<RelatedLinksPicker>`, `<MarkdownPreview>`

## Known caveats

1. **CSRF `state_mismatch` on dropped session cookie** — `/auth/login` stores state in session, `/auth/callback` compares. If the browser drops the cookie between redirects (cross-site cookie policy, incognito) the callback returns `/?error=state_mismatch`. Likely culprit: `SameSite=Lax` vs. GitHub redirect chain. Fix only when it bites.

2. **`SESSION_COOKIE_SECURE` on Railway not verified** — `main.rs` reads the env var; Railway is HTTPS so it should be `1`, but never confirmed in the dashboard. If login starts silently failing in prod, check this env var first.

3. **WASM bundle +80–120 KB from `pulldown-cmark`** — non-optional because the editor renders live preview client-side. If initial load feels slow, revert: make `pulldown-cmark` ssr-only and swap live preview for a debounced `render_markdown_preview` server fn.

4. **`prose-sm` typography sizing is a guess** — tune `tailwind.config.js` `typography.invert` palette and/or swap `prose-sm` → `prose-base` after seeing real content.

5. **Editor → reload-on-save is the update UX** — both Create and Update call `window.location.reload()` instead of invalidating the `graph` Resource. Simple, consistent, but costs a full SSR round-trip. Deferred 2026-04-18; fix scheduled in Phase 3.

6. **Update path regenerates frontmatter from templates** — if a doc has custom fields (e.g., `status: accepted` on an ADR past-draft), they are wiped on save. Body is preserved verbatim. Fix by round-tripping the parsed frontmatter dict instead of re-emitting from a template during updates.

7. **No auto-refresh after out-of-band commits** — the 30s TTL cache in `runtime.rs` bounds staleness for edits made via `git push` directly. Acceptable; documented here so symptom isn't mistaken for a bug.
