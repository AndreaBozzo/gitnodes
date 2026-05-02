use std::sync::OnceLock;

use brain_domain::BrainError;
use sqlx::SqlitePool;

mod bulk_insert;
mod files;
mod links;
mod migrations;
mod nodes;
mod rebuild;
mod sync_state;
mod target;
#[cfg(test)]
mod tests;
mod work_items;

pub use files::{FileFilters, ProjectedFile, list_files};
pub use migrations::migrate;
pub use nodes::{NodeFilters, list_nodes, read_node};
pub use rebuild::{load_graph, rebuild};
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
