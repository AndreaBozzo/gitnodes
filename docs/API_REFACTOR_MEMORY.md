# API Refactor Memory

Last updated: 2026-04-28

Purpose: split the growing `crates/brain-app/src/api.rs` module without changing
server function contracts, Leptos client imports, or release-mode registration.

## Current Shape

- `api.rs` remains the public index for server functions and shared response
  types. Re-exports siblings so external `use crate::api::{...}` paths keep
  working.
- `api/files.rs` owns file CRUD APIs and write contracts:
  `BrainFile`, `WriteMode`, `WriteResult`, `WriteCapabilities`,
  `ReadBrainFile`, `SaveBrainFile`, `DeleteBrainFile`, `GetWriteCapabilities`,
  frontmatter merge, related-section build/strip, and their tests.
- `api/file_ops.rs` owns rename, asset upload, folder list, and link helpers:
  `RenameBrainFile`, `UploadAsset`, `ListBrainFolders`, `RenameResult`,
  `rewrite_links`/`resolve_link_path`/`relativize`, `slugify`,
  `split_filename`, `is_allowed_image_ext`, `short_content_hash`,
  `MAX_ASSET_BYTES`, `perform_rename_on_storage`, `collect_repo_md_paths`,
  and `rewrite_links_tests`. `relativize` and `slugify` are `pub(crate)` and
  re-exported from `api` so siblings (`files.rs`, `write_orchestrator.rs`)
  keep their `super::relativize` / `super::slugify` paths.
- `api/write_orchestrator.rs` owns permission-aware direct-write vs PR fallback:
  save, delete, PR branch planning, fork branch path, PR opening, projection
  rebuild after writes.
- `api/work_items.rs` owns work-item read/mutation APIs:
  `ListWorkItems`, `LoadWorkItemByPath`, `LoadWorkItemComments`,
  `TransitionWorkItem`, `AssignWorkItem`, `BindWorkItem`, provider-originated
  issue reconciliation, provider label/state sync.
- `api/graph.rs` owns graph/projection read APIs:
  `ListNodes`, `ReadNode`, `LoadBrainGraph`, `RefreshBrainGraph`,
  `ListAccessibleTargets`, `LoadBrainGraphForTarget`,
  `LoadBrainConfigForTarget`, `NodeQueryFilters`, `AccessibleTarget`.
- `api/config_admin.rs` owns config/admin APIs:
  `GetAppConfig`, `LoadBrainConfig`, `ListViews`, `SaveViews`,
  `LoadAuditLog`, `ListSessions`, `RevokeSession`, `GetCurrentUser`,
  `LoadBrainTemplate`, `AppConfig`, `AuditEntry`, `SessionEntry`.

## Invariants

- Existing imports from `crate::api::{...}` must keep working.
- Every `#[server(...)]` type must stay listed in `SERVER_FNS` and explicitly
  registered in `register_server_functions`.
- The server function registration test must scan every file that can define
  `#[server]` functions in the `api` module tree (update `API_SOURCES` in
  `api.rs::server_fn_registration_tests` whenever a new submodule is added).
- PR fallback must not patch the target-branch projection until the PR lands.
- Direct writes must keep cache invalidation and projection rebuild behavior.
- Work-item provider sync failures are audited, not rolled back.
- Webhook provider-originated updates must avoid echoing changes back to GitHub.

## Verification Gate

Run after every slice:

```bash
cargo check -p brain-app --no-default-features --features ssr
cargo check -p brain-app --no-default-features --features hydrate --target wasm32-unknown-unknown
cargo test -p brain-app --no-default-features --features ssr
git diff --check
```

Run the full gate before merging larger batches:

```bash
just check
```

## Suggested Next Slices

1. ~~Extract file CRUD APIs (B1)~~ — done 2026-04-28 as `api/files.rs` covering
   read/save/delete + write types + frontmatter merge + related-section helpers.
2. ~~Extract rename + assets + folder list (B2)~~ — done 2026-04-28 as
   `api/file_ops.rs`. After B1+B2, `api.rs` shrank from 2039 to ~742 lines.
3. ~~Extract graph/projection read APIs~~ — done 2026-04-28 as `api/graph.rs`
   covering node filters/read side, graph load/refresh, target switcher
   discovery, and target-explicit graph/config reads.
4. ~~Extract config/admin APIs~~ — done 2026-04-28 as `api/config_admin.rs`
   covering app/config/view/session/audit/template server functions.
5. Revisit `SERVER_FNS` registration once modules stabilize; keep it boring and
   explicit unless a safer generated registry is introduced.
