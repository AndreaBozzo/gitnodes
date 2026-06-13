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

//! Outbox for best-effort provider pushes (Failure-Mode Matrix slice γ).
//!
//! When an editorial save with `system_of_record = split|external` propagates
//! to the forge and the push fails, the editorial save is kept (no rollback)
//! and a row is enqueued here. A supervised background job retries by
//! reconciling the provider to the Brain file's *current* state, so a stale
//! enqueued payload is never replayed. `kind` (`state`/`assignees`/`binding`)
//! selects which dimension the retry re-pushes.

use gitnodes_domain::BrainError;
use sqlx::{Row, SqlitePool};

use super::sqlx_error;

/// One un-propagated provider mutation, joined with its target identity. This
/// is the storage-layer record; the API layer maps it to the serializable
/// `PendingSyncRow` DTO returned to the admin UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingSyncRecord {
    pub id: i64,
    pub org: String,
    pub repo: String,
    pub branch: String,
    pub brain_id: String,
    pub kind: String,
    pub attempts: i64,
    pub last_attempt_at: String,
    pub last_error: Option<String>,
}

/// A pending row keyed for retry: identity needed to rebuild storage + reconcile.
#[cfg(feature = "ssr")]
#[derive(Clone, Debug)]
pub(crate) struct PendingSyncJob {
    pub id: i64,
    pub org: String,
    pub repo: String,
    pub branch: String,
    pub brain_id: String,
    pub kind: String,
}

/// Enqueue (or bump) a failed provider push. The `UNIQUE(target_id, brain_id,
/// kind)` constraint means a repeated failure of the same mutation increments
/// `attempts` instead of piling duplicate rows.
#[cfg(feature = "ssr")]
pub(crate) async fn enqueue(
    pool: &SqlitePool,
    target_id: i64,
    brain_id: &str,
    kind: &str,
    error: &str,
) -> Result<(), BrainError> {
    sqlx::query(
        "INSERT INTO pending_provider_sync (target_id, brain_id, kind, attempts, last_error)
         VALUES (?, ?, ?, 1, ?)
         ON CONFLICT(target_id, brain_id, kind) DO UPDATE SET
            attempts = pending_provider_sync.attempts + 1,
            last_attempt_at = CURRENT_TIMESTAMP,
            last_error = excluded.last_error",
    )
    .bind(target_id)
    .bind(brain_id)
    .bind(kind)
    .bind(error)
    .execute(pool)
    .await
    .map_err(sqlx_error)?;
    Ok(())
}

/// Mark a retry attempt that failed again: bump attempts + record the error.
#[cfg(feature = "ssr")]
pub(crate) async fn record_retry_failure(
    pool: &SqlitePool,
    id: i64,
    error: &str,
) -> Result<(), BrainError> {
    sqlx::query(
        "UPDATE pending_provider_sync
         SET attempts = attempts + 1, last_attempt_at = CURRENT_TIMESTAMP, last_error = ?
         WHERE id = ?",
    )
    .bind(error)
    .bind(id)
    .execute(pool)
    .await
    .map_err(sqlx_error)?;
    Ok(())
}

/// Remove a row once its mutation has successfully propagated (or is moot).
#[cfg(feature = "ssr")]
pub(crate) async fn delete(pool: &SqlitePool, id: i64) -> Result<(), BrainError> {
    sqlx::query("DELETE FROM pending_provider_sync WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(sqlx_error)?;
    Ok(())
}

/// Oldest-first batch of jobs to retry. Capped so a backlog can't make one
/// retry tick run unbounded; remaining rows are picked up next tick.
#[cfg(feature = "ssr")]
pub(crate) async fn next_batch(
    pool: &SqlitePool,
    limit: i64,
    max_attempts: i64,
) -> Result<Vec<PendingSyncJob>, BrainError> {
    let rows = sqlx::query(
        "SELECT p.id, t.org, t.repo, t.branch, p.brain_id, p.kind
         FROM pending_provider_sync p
         JOIN targets t ON t.id = p.target_id
         WHERE p.attempts < ?
         ORDER BY p.last_attempt_at ASC
         LIMIT ?",
    )
    .bind(max_attempts)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(sqlx_error)?;

    Ok(rows
        .into_iter()
        .map(|row| PendingSyncJob {
            id: row.get("id"),
            org: row.get("org"),
            repo: row.get("repo"),
            branch: row.get("branch"),
            brain_id: row.get("brain_id"),
            kind: row.get("kind"),
        })
        .collect())
}

/// Full list for the read-only admin surface, newest activity first.
#[cfg(feature = "ssr")]
pub async fn list_all(pool: &SqlitePool, limit: i64) -> Result<Vec<PendingSyncRecord>, BrainError> {
    let rows = sqlx::query(
        "SELECT p.id, t.org, t.repo, t.branch, p.brain_id, p.kind, p.attempts,
                p.last_attempt_at, p.last_error
         FROM pending_provider_sync p
         JOIN targets t ON t.id = p.target_id
         ORDER BY p.last_attempt_at DESC
         LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(sqlx_error)?;

    Ok(rows
        .into_iter()
        .map(|row| PendingSyncRecord {
            id: row.get("id"),
            org: row.get("org"),
            repo: row.get("repo"),
            branch: row.get("branch"),
            brain_id: row.get("brain_id"),
            kind: row.get("kind"),
            attempts: row.get("attempts"),
            last_attempt_at: row.get("last_attempt_at"),
            last_error: row.get("last_error"),
        })
        .collect())
}
