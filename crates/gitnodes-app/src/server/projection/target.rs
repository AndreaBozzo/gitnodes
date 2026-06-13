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

use gitnodes_domain::{BrainError, TargetConfig, TargetKey};
use sqlx::{Row, SqlitePool};

use super::sqlx_error;

pub(super) async fn ensure_target_id(
    pool: &SqlitePool,
    target: &TargetConfig,
) -> Result<i64, BrainError> {
    let key = TargetKey::from(target);
    sqlx::query(
        "INSERT INTO targets (key, org, repo, branch, registered_at, default_branch)
         VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP, ?)
         ON CONFLICT(org, repo) DO UPDATE SET
            key = excluded.key,
            branch = excluded.branch,
            default_branch = COALESCE(targets.default_branch, excluded.default_branch)",
    )
    .bind(key.as_str())
    .bind(&target.org)
    .bind(&target.repo)
    .bind(&target.branch)
    .bind(&target.branch)
    .execute(pool)
    .await
    .map_err(sqlx_error)?;

    let row = sqlx::query("SELECT id FROM targets WHERE org = ? AND repo = ?")
        .bind(&target.org)
        .bind(&target.repo)
        .fetch_one(pool)
        .await
        .map_err(sqlx_error)?;
    Ok(row.get::<i64, _>("id"))
}
