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

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::OnceLock;

static POOL: OnceLock<SqlitePool> = OnceLock::new();

pub fn init(pool: SqlitePool) {
    let _ = POOL.set(pool);
}

fn pool() -> Option<&'static SqlitePool> {
    POOL.get()
}

/// Fire-and-forget: log an event. Failures are swallowed so auth/CRUD paths
/// never fail because of logging.
pub async fn log(kind: &str, actor: Option<&str>, detail: &str) {
    let Some(pool) = pool() else { return };
    let _ = sqlx::query("INSERT INTO audit_events (kind, actor, detail) VALUES (?, ?, ?)")
        .bind(kind)
        .bind(actor)
        .bind(detail)
        .execute(pool)
        .await;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditRow {
    pub id: i64,
    pub ts: String,
    pub kind: String,
    pub actor: Option<String>,
    pub detail: Option<String>,
}

pub async fn recent(limit: i64, kind_filter: Option<&str>) -> Result<Vec<AuditRow>, sqlx::Error> {
    let Some(pool) = pool() else {
        return Ok(vec![]);
    };
    let rows = if let Some(k) = kind_filter {
        sqlx::query_as::<_, (i64, String, String, Option<String>, Option<String>)>(
            "SELECT id, ts, kind, actor, detail FROM audit_events WHERE kind = ? ORDER BY id DESC LIMIT ?",
        )
        .bind(k)
        .bind(limit)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, (i64, String, String, Option<String>, Option<String>)>(
            "SELECT id, ts, kind, actor, detail FROM audit_events ORDER BY id DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(pool)
        .await?
    };
    Ok(rows
        .into_iter()
        .map(|(id, ts, kind, actor, detail)| AuditRow {
            id,
            ts,
            kind,
            actor,
            detail,
        })
        .collect())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionRow {
    pub id: String,
    pub expiry_date: String,
}

pub async fn list_sessions(limit: i64) -> Result<Vec<SessionRow>, sqlx::Error> {
    let Some(pool) = pool() else {
        return Ok(vec![]);
    };
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT id, expiry_date FROM tower_sessions ORDER BY expiry_date DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, expiry_date)| SessionRow { id, expiry_date })
        .collect())
}

pub async fn revoke_session(id: &str) -> Result<u64, sqlx::Error> {
    let Some(pool) = pool() else { return Ok(0) };
    let res = sqlx::query("DELETE FROM tower_sessions WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}
