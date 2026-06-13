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

use gitnodes_domain::BrainError;
use sqlx::{Row, SqlitePool};

use super::sqlx_error;

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug)]
pub(super) struct SyncState {
    pub status: String,
    pub last_success_at: Option<String>,
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) async fn load_sync_state(
    pool: &SqlitePool,
    target_id: i64,
) -> Result<Option<SyncState>, BrainError> {
    let row = sqlx::query(
        "SELECT status, last_success_at FROM projection_sync_state WHERE target_id = ?",
    )
    .bind(target_id)
    .fetch_optional(pool)
    .await
    .map_err(sqlx_error)?;

    Ok(row.map(|row| SyncState {
        status: row.get::<String, _>("status"),
        last_success_at: row.get::<Option<String>, _>("last_success_at"),
    }))
}

pub(super) async fn record_attempt(
    pool: &SqlitePool,
    target_id: i64,
    reason: &str,
) -> Result<(), BrainError> {
    sqlx::query(
        "INSERT INTO projection_sync_state (
            target_id, status, last_attempt_at, last_reason, file_count, node_count, edge_count
        ) VALUES (?, 'running', CURRENT_TIMESTAMP, ?, 0, 0, 0)
        ON CONFLICT(target_id) DO UPDATE SET
            status = 'running',
            last_attempt_at = CURRENT_TIMESTAMP,
            last_reason = excluded.last_reason,
            last_error = NULL",
    )
    .bind(target_id)
    .bind(reason)
    .execute(pool)
    .await
    .map_err(sqlx_error)?;
    Ok(())
}

pub(super) async fn record_failure(
    pool: &SqlitePool,
    target_id: i64,
    reason: &str,
    error: &str,
) -> Result<(), BrainError> {
    sqlx::query(
        "INSERT INTO projection_sync_state (
            target_id, status, last_attempt_at, last_error_at, last_error, last_reason, file_count, node_count, edge_count
        ) VALUES (?, 'error', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, ?, ?, 0, 0, 0)
        ON CONFLICT(target_id) DO UPDATE SET
            status = 'error',
            last_attempt_at = CURRENT_TIMESTAMP,
            last_error_at = CURRENT_TIMESTAMP,
            last_error = excluded.last_error,
            last_reason = excluded.last_reason",
    )
    .bind(target_id)
    .bind(error)
    .bind(reason)
    .execute(pool)
    .await
    .map_err(sqlx_error)?;
    Ok(())
}
