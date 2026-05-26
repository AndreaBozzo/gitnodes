use std::sync::OnceLock;

use brain_domain::BrainError;
use sqlx::SqlitePool;

mod bulk_insert;
mod files;
mod links;
mod migrations;
mod nodes;
pub mod pending_sync;
mod rebuild;
mod search;
mod sync_state;
mod target;
#[cfg(test)]
mod tests;
mod work_items;

pub use files::{FileFilters, ProjectedFile, list_files};
pub use migrations::migrate;
pub use nodes::{NodeFilters, list_nodes, read_node};
pub use pending_sync::PendingSyncRecord;
pub use rebuild::{load_graph, rebuild};
pub use search::{SearchFilters, SearchHit, search_nodes};
pub use work_items::{
    WorkItemFilters, find_work_item_by_external, list_work_items, load_work_item_by_brain_id,
    load_work_item_by_path, read_work_item_by_path, update_work_item_assignees,
    update_work_item_state, upsert_work_item_binding,
};

static POOL: OnceLock<SqlitePool> = OnceLock::new();
const SQLITE_MAX_VARIABLES: usize = 900;

pub fn init(pool: SqlitePool) {
    let _ = POOL.set(pool);
}

fn pool() -> Result<&'static SqlitePool, BrainError> {
    POOL.get()
        .ok_or_else(|| BrainError::other("Projection SQLite pool not initialized"))
}

/// Expose the projection pool to other server modules (`routing`,
/// server fns that need to call `target_registry::register_or_get`). Returns
/// `None` if `init` has not run yet - callers should treat that as "no
/// per-target sticky state available; behave as the env-default deploy".
pub fn pool_handle() -> Option<&'static SqlitePool> {
    POOL.get()
}

/// Enqueue a failed best-effort provider push into the outbox (slice γ).
/// Resolves `target_id` from the target identity, creating the row lazily via
/// the same `ensure_target_id` path the projection uses. Best-effort itself:
/// callers log on error rather than failing the editorial save.
pub async fn enqueue_pending_sync(
    target: &brain_domain::TargetConfig,
    brain_id: &str,
    kind: &str,
    error: &str,
) -> Result<(), BrainError> {
    let pool = pool()?;
    let target_id = target::ensure_target_id(pool, target).await?;
    pending_sync::enqueue(pool, target_id, brain_id, kind, error).await
}

/// Read-only listing for the admin "Pending provider sync" surface.
pub async fn list_all_pending_sync(limit: i64) -> Result<Vec<PendingSyncRecord>, BrainError> {
    pending_sync::list_all(pool()?, limit).await
}

/// Per-target projection state for the admin status surface. Joins the
/// `projection_sync_state` watermark with target identity and adds a
/// best-effort work_item_count so operators see the same numbers the rebuild
/// step recorded.
#[derive(Clone, Debug)]
pub struct ProjectionStatusRow {
    pub org: String,
    pub repo: String,
    pub branch: String,
    pub status: String,
    pub last_attempt_at: Option<String>,
    pub last_success_at: Option<String>,
    pub last_error_at: Option<String>,
    pub last_error: Option<String>,
    pub last_reason: Option<String>,
    pub file_count: i64,
    pub node_count: i64,
    pub edge_count: i64,
    pub work_item_count: i64,
    pub last_rebuild_duration_ms: Option<i64>,
}

/// Schema version (max applied migration) plus per-target rows. Used by the
/// admin status surface; webhook lag and rate-limit snapshot are deferred.
pub async fn projection_status() -> Result<(i64, Vec<ProjectionStatusRow>), BrainError> {
    use sqlx::Row;
    let pool = pool()?;

    let schema_version: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations")
            .fetch_one(pool)
            .await
            .map_err(sqlx_error)?;

    let rows = sqlx::query(
        "SELECT
            t.org, t.repo, t.branch,
            COALESCE(s.status, 'stale') AS status,
            s.last_attempt_at, s.last_success_at, s.last_error_at,
            s.last_error, s.last_reason,
            COALESCE(s.file_count, 0) AS file_count,
            COALESCE(s.node_count, 0) AS node_count,
            COALESCE(s.edge_count, 0) AS edge_count,
            s.last_rebuild_duration_ms,
            (SELECT COUNT(*) FROM work_items w WHERE w.target_id = t.id) AS work_item_count
         FROM targets t
         LEFT JOIN projection_sync_state s ON s.target_id = t.id
         ORDER BY t.org, t.repo, t.branch",
    )
    .fetch_all(pool)
    .await
    .map_err(sqlx_error)?;

    let status_rows = rows
        .into_iter()
        .map(|row| ProjectionStatusRow {
            org: row.get("org"),
            repo: row.get("repo"),
            branch: row.get("branch"),
            status: row.get("status"),
            last_attempt_at: row.get("last_attempt_at"),
            last_success_at: row.get("last_success_at"),
            last_error_at: row.get("last_error_at"),
            last_error: row.get("last_error"),
            last_reason: row.get("last_reason"),
            file_count: row.get("file_count"),
            node_count: row.get("node_count"),
            edge_count: row.get("edge_count"),
            work_item_count: row.get("work_item_count"),
            last_rebuild_duration_ms: row.get("last_rebuild_duration_ms"),
        })
        .collect();
    Ok((schema_version, status_rows))
}

fn normalize_path_prefix(prefix: &str) -> String {
    let trimmed = prefix.trim().trim_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}/")
    }
}

fn enum_str<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value)
        .unwrap_or_else(|_| "\"\"".to_string())
        .trim_matches('"')
        .to_string()
}

fn parse_enum_str<T>(raw: &str) -> Result<T, BrainError>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_str::<T>(&format!("\"{raw}\""))
        .map_err(|error| BrainError::parse(format!("projection enum parse: {error}")))
}

fn sqlx_error(error: sqlx::Error) -> BrainError {
    BrainError::Io(format!("projection sqlite: {error}"))
}
