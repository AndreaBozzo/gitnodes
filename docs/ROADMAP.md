# Brain_UI — Roadmap & Known Caveats

Tracking consolidation work and deferred items. Living document.

## In flight — Consolidation (2026-04)

Three-phase cleanup before adding new features. See plan in repo/PR history.

- **Phase 1 — Scaffolding & safety net** (current)
  - [x] CI workflow (`fmt`, `clippy` ssr+hydrate, `test`, Tailwind build)
  - [x] `justfile` for dev/lint/test/build/docker
  - [x] Structured logging via `tracing` (SSR)
  - [x] This roadmap
- **Phase 2 — Workspace & module boundaries**
  - [ ] Split into `brain-domain`, `brain-storage`, `brain-graph`, `brain-auth`, `brain-app`
  - [ ] `Storage` trait collapsing the 5× Octocrab boilerplate in `api.rs`
  - [ ] Domain `BrainError` enum replacing stringly `ServerFnError`
  - [ ] Unit tests for parsing / graph build / layout (pure crates)
- **Phase 3 — UI consolidation**
  - [ ] Shared `<Badge>` / `<Tag>` component
  - [ ] Accent color via CSS var instead of inline `style`
  - [ ] Remove `rel="external"` on internal `/admin` link (`page.rs:98`)
  - [ ] `graph_version: RwSignal<u64>` to replace `window.location.reload()`
  - [ ] Decompose `editor.rs` (469 LOC) into `<FrontmatterFields>`, `<TagInput>`, `<RelatedLinksPicker>`, `<MarkdownPreview>`

## Known caveats

1. **CSRF `state_mismatch` on dropped session cookie** — `/auth/login` stores state in session, `/auth/callback` compares. If the browser drops the cookie between redirects (cross-site cookie policy, incognito) the callback returns `/?error=state_mismatch`. Likely culprit: `SameSite=Lax` vs. GitHub redirect chain. Fix only when it bites.

2. **`SESSION_COOKIE_SECURE` on Railway not verified** — `main.rs` reads the env var; Railway is HTTPS so it should be `1`, but never confirmed in the dashboard. If login starts silently failing in prod, check this env var first.

3. **WASM bundle +80–120 KB from `pulldown-cmark`** — non-optional because the editor renders live preview client-side. If initial load feels slow, revert: make `pulldown-cmark` ssr-only and swap live preview for a debounced `render_markdown_preview` server fn.

4. **`prose-sm` typography sizing is a guess** — tune `tailwind.config.js` `typography.invert` palette and/or swap `prose-sm` → `prose-base` after seeing real content.

5. **Editor → reload-on-save is the update UX** — both Create and Update call `window.location.reload()` instead of invalidating the `graph` Resource. Simple, consistent, but costs a full SSR round-trip. Deferred 2026-04-18; fix scheduled in Phase 3.

6. **Update path regenerates frontmatter from templates** — if a doc has custom fields (e.g., `status: accepted` on an ADR past-draft), they are wiped on save. Body is preserved verbatim. Fix by round-tripping the parsed frontmatter dict instead of re-emitting from a template during updates.

7. **No auto-refresh after out-of-band commits** — the 30s TTL cache in `runtime.rs` bounds staleness for edits made via `git push` directly. Acceptable; documented here so symptom isn't mistaken for a bug.
