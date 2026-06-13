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

use gitnodes_domain::{BrainError, TargetConfig};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};

use super::{normalize_path_prefix, pool, sqlx_error, target::ensure_target_id};

#[derive(Clone, Debug, Default)]
pub struct FileFilters {
    pub path_prefix: Option<String>,
    pub orphan_only: bool,
}

#[derive(Clone, Debug)]
pub struct ProjectedFile {
    pub path: String,
    pub sha: String,
    pub size_bytes: i64,
    pub node_type: Option<String>,
    pub title: Option<String>,
    pub is_work_item: bool,
    pub is_orphan_in_graph: bool,
}

pub async fn list_files(
    target: &TargetConfig,
    filters: &FileFilters,
) -> Result<Vec<ProjectedFile>, BrainError> {
    let pool = pool()?;
    let target_id = ensure_target_id(pool, target).await?;
    list_files_from_pool(pool, target_id, filters).await
}

pub(super) async fn list_files_from_pool(
    pool: &SqlitePool,
    target_id: i64,
    filters: &FileFilters,
) -> Result<Vec<ProjectedFile>, BrainError> {
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT
            f.path,
            f.sha,
            f.size_bytes,
            n.node_type,
            n.title,
            wi.brain_id AS work_item_brain_id,
            EXISTS (
                SELECT 1 FROM backlinks b
                WHERE b.target_id = f.target_id
                  AND (b.source_path = f.path OR b.target_path = f.path)
            ) AS has_graph_link
         FROM files f
         LEFT JOIN nodes n
            ON n.target_id = f.target_id
           AND n.path = f.path
           AND n.is_virtual = 0
         LEFT JOIN work_items wi
            ON wi.target_id = f.target_id
           AND wi.content_path = f.path
         WHERE f.target_id = ",
    );
    query.push_bind(target_id);

    if let Some(prefix) = filters
        .path_prefix
        .as_deref()
        .map(normalize_path_prefix)
        .filter(|p| !p.is_empty())
    {
        query.push(" AND f.path LIKE ");
        query.push_bind(format!("{prefix}%"));
    }
    if filters.orphan_only {
        query.push(
            " AND NOT EXISTS (
                SELECT 1 FROM backlinks b
                WHERE b.target_id = f.target_id
                  AND (b.source_path = f.path OR b.target_path = f.path)
            )",
        );
    }
    query.push(" ORDER BY f.path ASC");

    let rows = query.build().fetch_all(pool).await.map_err(sqlx_error)?;
    Ok(rows
        .into_iter()
        .map(|row| ProjectedFile {
            path: row.get::<String, _>("path"),
            sha: row.get::<String, _>("sha"),
            size_bytes: row.get::<i64, _>("size_bytes"),
            node_type: row.get::<Option<String>, _>("node_type"),
            title: row.get::<Option<String>, _>("title"),
            is_work_item: row.get::<Option<String>, _>("work_item_brain_id").is_some(),
            is_orphan_in_graph: row.get::<i64, _>("has_graph_link") == 0,
        })
        .collect())
}
