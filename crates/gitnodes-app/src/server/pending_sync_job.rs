// Copyright (C) 2026 Andrea Bozzo
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

//! Background retry job for the provider-sync outbox (Failure-Mode Matrix
//! slice γ).
//!
//! A single supervised tokio task polls `pending_provider_sync` on an interval
//! and tries to reconcile each pending work item's provider state to the Brain
//! file's *current* truth. It authenticates as the GitHub App (App-first, PAT
//! fallback) since there's no user session in the background — the retry commit
//! is therefore App-attributed. On success the row is deleted; on failure the
//! attempt count is bumped and the row stays for the next tick.
//!
//! Deliberately lightweight: no job framework, just `tokio::time::interval` +
//! the same `spawn_supervised`-style panic logging used by webhooks. Per the
//! roadmap, this stays as-is until SQLite + a supervised task no longer suffice.

use std::time::Duration;

use gitnodes_storage::{GithubHttp, GithubStorage};
use sqlx::SqlitePool;

use super::projection::pending_sync;

/// Default poll interval. Provider outages are usually minutes-long, so a tight
/// loop buys nothing; 60s keeps the retry responsive without hammering GitHub.
const DEFAULT_INTERVAL_SECS: u64 = 60;
/// Max rows reconciled per tick, so a large backlog can't make one tick run
/// unbounded. Remaining rows are picked up on the next tick.
const BATCH_LIMIT: i64 = 25;
/// Stop retrying after this many attempts and leave the row for an operator to
/// inspect in admin — a row failing this many times is a real problem (renamed
/// file, revoked binding, permanent permission loss), not a transient outage.
pub(crate) const MAX_ATTEMPTS: i64 = 20;

/// Spawn the supervised retry loop. Returns immediately; the task runs for the
/// process lifetime. If the projection pool isn't initialized the task is a
/// no-op (logged once).
pub fn spawn(pool: SqlitePool, http: GithubHttp) {
    let interval_secs = std::env::var("PENDING_SYNC_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_INTERVAL_SECS);

    let task = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(interval_secs));
        loop {
            ticker.tick().await;
            if let Err(error) = run_once(&pool, &http).await {
                tracing::warn!(%error, "pending-sync retry tick failed");
            }
        }
    });

    // Supervise: a panic in the loop should be logged loudly, not silently
    // detached. Mirrors `webhook::spawn_supervised`. The loop never returns
    // `Ok`, so we only care about the `Err` (panic/cancel) arm.
    tokio::spawn(async move {
        match task.await {
            Ok(()) => {}
            Err(join_err) if join_err.is_panic() => {
                tracing::error!("pending-sync retry loop panicked");
            }
            Err(_) => {
                tracing::warn!("pending-sync retry loop cancelled");
            }
        }
    });
}

/// One retry pass over a batch of pending rows.
async fn run_once(pool: &SqlitePool, http: &GithubHttp) -> Result<(), gitnodes_domain::BrainError> {
    let batch = pending_sync::next_batch(pool, BATCH_LIMIT, MAX_ATTEMPTS).await?;
    if batch.is_empty() {
        return Ok(());
    }

    // One token for the whole batch; webhooks use the same App-first resolution.
    let token = match crate::server::installation_token::get(http).await {
        Some(t) => t,
        None => {
            tracing::warn!(
                pending = batch.len(),
                "pending-sync retry: no GitHub App or PAT credentials — leaving rows for later"
            );
            return Ok(());
        }
    };

    for job in batch {
        if let Err(error) = reconcile_one(http, &token, &job).await {
            let detail = error.to_string();
            tracing::warn!(brain_id = %job.brain_id, error = %detail, "pending-sync retry: reconcile failed");
            let _ = pending_sync::record_retry_failure(pool, job.id, &detail).await;
        } else {
            tracing::info!(brain_id = %job.brain_id, "pending-sync retry: reconciled, clearing row");
            let _ = pending_sync::delete(pool, job.id).await;
        }
    }
    Ok(())
}

/// Reconcile one pending row: re-push the failed dimension (per `kind`) from the
/// work item's *current* Brain state to the provider. This is the **outbound**
/// direction — the same `sync_work_item_provider` push that originally failed —
/// not the inbound provider→Brain path. Idempotent: if a later edit already
/// propagated, the push re-asserts the same value and the row clears.
async fn reconcile_one(
    http: &GithubHttp,
    token: &str,
    job: &pending_sync::PendingSyncJob,
) -> Result<(), gitnodes_domain::BrainError> {
    use gitnodes_domain::TargetConfig;

    let target = TargetConfig {
        org: job.org.clone(),
        repo: job.repo.clone(),
        branch: job.branch.clone(),
    };
    let storage = GithubStorage::new(http.clone(), target.clone());

    crate::api::reconcile_provider_sync(
        token,
        "gitnodes-app[bot]",
        &target,
        &storage,
        &job.brain_id,
        &job.kind,
    )
    .await
}
