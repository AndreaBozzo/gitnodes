# Insights: Omnigraph Comparison → Brain UI

## Context

An external comparison (Gemini-generated, 2026-05-25) put Brain UI side-by-side with [ModernRelay/omnigraph](https://github.com/ModernRelay/omnigraph), a Lance/DataFusion embedded graph storage engine. The two systems are inverse halves of the problem space — Brain UI is a workspace+UI over Git as source of truth; Omnigraph is a columnar storage engine with branches living inside the engine. Most Omnigraph patterns don't apply directly, but a small number translate cleanly *if filtered against Brain UI's stated values* (Git as SoT, zero lock-in, no embedded vector store, merge offloaded to GitHub PR on purpose).

This doc records which Omnigraph insights survive that filter and which don't. The Omnigraph patterns referenced below were verified by reading the actual repo (manifest recovery, three-way merge, Cedar policy crate, Pest `.gq` grammar + RRF fusion — all real and substantive there). Where Gemini's framing was wrong, this doc says so explicitly rather than carrying the error forward.

## What we keep as-is, and why

- **Git remains source of truth.** [README.md](../README.md) line 9. Zero-lock-in is the product.
- **Merge stays offloaded to GitHub PR.** [crates/brain-app/src/api/files.rs](../crates/brain-app/src/api/files.rs). Bringing an in-engine merger would duplicate GitHub.
- **Permissions stay centralized at `WriteCapabilities`.** Phase 3.3 already shipped this — Gemini's "scattered at Axum boundary" framing is outdated.
- **No vector / no embeddings.** [docs/ROADMAP.md](ROADMAP.md) explicitly chose FTS5 as the search ceiling.
- **No precommit merge engine in the projection.** [crates/brain-storage/src/git_transaction.rs](../crates/brain-storage/src/git_transaction.rs) — optimistic `BrainError::Conflict` is the conscious posture.

## Insights worth adopting (ranked by ROI)

### 3.1 — Content-hash signatures for projection drift detection
**Size:** S. **Suggested home:** Schema v2 follow-up (post-landed, open).

*What:* add a `blob_sha` column per projected row; on rebuild, diff Git tree SHAs vs. projection SHAs to find stale rows in O(changed) instead of O(repo).

*Touches:* [crates/brain-app/src/server/projection/rebuild.rs](../crates/brain-app/src/server/projection/rebuild.rs), [crates/brain-app/migrations/0002_projection_v2.sql](../crates/brain-app/migrations/0002_projection_v2.sql) (extend).

*Why here:* rebuild today is full-upsert. Operators can't tell "what drifted" from "what we re-wrote". One TEXT column + one diff pass pays off as a debugging tool *and* as a perf hatch. Schema v2 already reserves the adjacent columns, so this rides on landed migration infra and de-risks the empty `body_text` / `frontmatter_json` slots by giving us a way to *see* drift.

*Smallest spike:* add `blob_sha` to `files` / `nodes`, populate during rebuild, log changed-set size per rebuild. Gate any incremental-rebuild behavior on a later phase.

### 3.2 — Typed conflict enum for write-path
**Size:** S. **Suggested home:** hardening lane (prerequisite for 4.4).

*What:* replace `BrainError::Conflict(String)` with a typed enum (`PathTaken`, `BlobShaMoved`, `RefNonFastForward`, `RemotePathDeletedUnderUs`).

*Touches:* [crates/brain-domain/src/error.rs](../crates/brain-domain/src/error.rs), [crates/brain-storage/src/git_transaction.rs](../crates/brain-storage/src/git_transaction.rs), [crates/brain-app/src/api/files.rs](../crates/brain-app/src/api/files.rs).

*Why here:* `git_transaction.rs` *already distinguishes* these cases internally (see the module header comment on the two race classes) — we throw the information away at the boundary. Omnigraph's `MergeConflictKind` enum is the right *shape* (we do not adopt their in-engine merge). Pairs naturally with the recently-shipped `ApiError` typing and is a hard prerequisite for 4.4 Advanced Conflict Resolution.

*Smallest spike:* keep the existing string message, add a parallel `kind: ConflictKind` field, surface it once in the toast UI and in the audit log.

### 3.3 — Sidecar-based outbox recovery
**Size:** M. **Suggested home:** Phase 4 candidate (prerequisite for 4.5).

*What:* borrow Omnigraph's manifest-recovery discipline (`__recovery/{ulid}.json` declaring intent before commit). Apply to `pending_provider_sync`: write the intended provider mutation as an intent journal *before* the GitHub call, advance state on confirmation. A coordinator classifies drift (`AlreadyApplied`, `NoMovement`, `RemoteDiverged`, `InvariantViolation`) on restart.

*Touches:* [crates/brain-app/src/server/pending_sync_job.rs](../crates/brain-app/src/server/pending_sync_job.rs), [crates/brain-app/src/server/projection/pending_sync.rs](../crates/brain-app/src/server/projection/pending_sync.rs), new migration.

*Why here:* current flow (`MAX_ATTEMPTS=20` → preserve row for operator) is *detection without classification*. An operator at attempt 20 has no machine-checkable answer to "did this already apply on the remote?" — they must read the issue by hand. We are *not* adopting Omnigraph's full 3-phase commit; we adopt the **intent-journal discipline** that makes recovery decidable. Connects directly to 4.0 `BranchTransaction` and 4.5 auto-binding work item ↔ issue (both add multi-step provider writes where mid-flight failure becomes harder to reason about).

*Smallest spike:* add `intent_payload TEXT` + `phase TEXT CHECK(phase IN ('declared','applied','confirmed'))` to `pending_provider_sync`, log transitions, surface in admin.

### 3.4 — RRF fusion for hybrid search (no scope creep)
**Size:** part of the existing FTS5 item — not a new line.

*What:* when FTS5 lands, score results by **Reciprocal Rank Fusion (k=60)** of two cheap rankers: FTS5/BM25 over `body_text` + structured-filter overlap (tag/type/path-prefix match count). **No vector backend.**

*Touches:* extends the existing "Full-Text Search (FTS5)" spec in Future Product Expansion.

*Why here:* Omnigraph's hybrid-search insight is **RRF**, not vectors. RRF only needs ranked lists. We already produce one (filter overlap). Adding FTS5 gives us the second. ~15-line fusion function — no model, no embeddings, no daemon. This is the version of hybrid search that respects zero-lock-in.

*Smallest spike:* prototype RRF over two synthetic ranked lists in a unit test before FTS5 schema work begins, to lock the API contract.

### 3.5 — Snapshot-pinned projection reads
**Size:** S. **Suggested home:** parking lot.

*What:* SQLite `BEGIN DEFERRED` / `sqlite3_snapshot_open` to pin the read view for the duration of a long graph render, so a webhook-driven rebuild mid-paint can't observe partially-updated state.

*Why parked:* no reproducer in dogfooding yet. Don't pay the read-path complexity until someone hits it. Becomes real with multi-user collaboration scale. Tracked under Phase 6.3 with an explicit trigger condition.

## Explicitly rejected (with reasoning)

- **Lance / embedded vector store.** Violates Git-as-SoT and zero-lock-in. The projection must remain rebuildable from `git clone` alone.
- **In-engine three-way merge.** Brain UI offloads divergent edits to GitHub PR *on purpose*. An in-engine merger duplicates GitHub and creates a second conflict-resolution UX nobody asked for.
- **Cedar policy engine.** `WriteCapabilities` is a 4-field struct derived from one GitHub call. Cedar buys nothing until policies become multi-actor + multi-resource + offline-evaluable. Adopting it now is pure cargo cult.
- **Embedded vector ANN / embeddings.** Adds model + runtime dependency, breaks reproducibility from Git alone, solves a recall problem nobody has filed. (Re-visitable under Phase 6.6 with explicit triggers.)
- **`.gq`-style Pest query DSL.** Filter panel handles single-axis composition fine today. Adopt only when saved views need OR/NOT composition — pre-building over-fits on imagined usage.
- **Generalized `PolicyProvider` trait.** Same logic as the deferred `ForgeAdapter`: designing against one provider produces the wrong shape. Gated on a real second forge under Phase 6.4.

## Suggested next step

Slot **3.1** and **3.2** into the next post-dogfooding feature slice. Both are S-sized, neither adds a dependency, and both make later work cheaper: 3.1 is a precondition for any incremental rebuild story and de-risks Schema v2's empty `body_text` / `frontmatter_json` columns; 3.2 is the cheapest observability win in the write path and is a hard prerequisite for 4.4 Advanced Conflict Resolution. One PR validates both: add `blob_sha` to `files` / `nodes` and populate from rebuild; in the same PR introduce `ConflictKind` next to `BrainError::Conflict` and tag the three throw sites in `git_transaction.rs`. Promote 3.3 to a real Phase 4 line item iff the typed-conflict telemetry reveals a non-trivial provider-sync drift class.
