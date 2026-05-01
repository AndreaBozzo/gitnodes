use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;

use brain_domain::{
    BrainConfig, BrainError, Edge, ExternalWorkItemBinding, Node, TargetConfig, TargetKey,
    WorkItem, WorkItemKind, WorkItemState, WorkItemSystemOfRecord,
};
use brain_graph::{RawFile, build_graph, parse_file};
use brain_storage::GithubStorage;
use serde_yaml::Value;
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool, sqlite::SqliteRow};

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
/// `None` if `init` has not run yet — callers should treat that as "no
/// per-target sticky state available; behave as the env-default deploy".
pub fn pool_handle() -> Option<&'static SqlitePool> {
    POOL.get()
}

pub async fn migrate(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(pool)
        .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS targets (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            key TEXT NOT NULL UNIQUE,
            org TEXT NOT NULL,
            repo TEXT NOT NULL,
            branch TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    // 3.7B-α: extend `targets` with registration metadata so a deterministic
    // sticky branch can be persisted at first sighting of (org, repo). The
    // ALTERs are guarded by `pragma_table_info` because SQLite has no
    // `ADD COLUMN IF NOT EXISTS`. UNIQUE(org, repo) enforces the stickiness
    // invariant — one branch per repo per deployment, decided at first
    // registration. Re-registration is a separate explicit mutation (β).
    add_column_if_missing(
        pool,
        "targets",
        "registered_at",
        "TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP",
    )
    .await?;
    add_column_if_missing(pool, "targets", "registered_by", "TEXT").await?;
    add_column_if_missing(
        pool,
        "targets",
        "source",
        "TEXT NOT NULL DEFAULT 'env_default'",
    )
    .await?;
    add_column_if_missing(pool, "targets", "default_branch", "TEXT").await?;
    sqlx::query("UPDATE targets SET default_branch = branch WHERE default_branch IS NULL")
        .execute(pool)
        .await?;
    sqlx::query(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_targets_org_repo
            ON targets(org, repo)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS projection_sync_state (
            target_id INTEGER PRIMARY KEY,
            status TEXT NOT NULL DEFAULT 'stale',
            last_attempt_at TEXT,
            last_success_at TEXT,
            last_error_at TEXT,
            last_error TEXT,
            last_reason TEXT,
            file_count INTEGER NOT NULL DEFAULT 0,
            node_count INTEGER NOT NULL DEFAULT 0,
            edge_count INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY(target_id) REFERENCES targets(id) ON DELETE CASCADE
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS files (
            target_id INTEGER NOT NULL,
            path TEXT NOT NULL,
            sha TEXT NOT NULL,
            size_bytes INTEGER NOT NULL,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (target_id, path),
            FOREIGN KEY(target_id) REFERENCES targets(id) ON DELETE CASCADE
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS nodes (
            target_id INTEGER NOT NULL,
            node_id INTEGER NOT NULL,
            title TEXT NOT NULL,
            summary TEXT NOT NULL,
            node_type TEXT NOT NULL,
            tags_json TEXT NOT NULL,
            x REAL NOT NULL,
            y REAL NOT NULL,
            path TEXT NOT NULL,
            sha TEXT NOT NULL,
            is_virtual INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (target_id, node_id),
            FOREIGN KEY(target_id) REFERENCES targets(id) ON DELETE CASCADE
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS edges (
            target_id INTEGER NOT NULL,
            from_id INTEGER NOT NULL,
            to_id INTEGER NOT NULL,
            PRIMARY KEY (target_id, from_id, to_id),
            FOREIGN KEY(target_id) REFERENCES targets(id) ON DELETE CASCADE
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS backlinks (
            target_id INTEGER NOT NULL,
            source_path TEXT NOT NULL,
            target_path TEXT NOT NULL,
            PRIMARY KEY (target_id, source_path, target_path),
            FOREIGN KEY(target_id) REFERENCES targets(id) ON DELETE CASCADE
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS work_items (
            target_id INTEGER NOT NULL,
            brain_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            title TEXT NOT NULL,
            state TEXT NOT NULL,
            labels_json TEXT NOT NULL DEFAULT '[]',
            assignees_json TEXT NOT NULL DEFAULT '[]',
            content_path TEXT,
            system_of_record TEXT NOT NULL DEFAULT 'brain',
            PRIMARY KEY (target_id, brain_id),
            FOREIGN KEY(target_id) REFERENCES targets(id) ON DELETE CASCADE
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS work_item_bindings (
            target_id INTEGER NOT NULL,
            brain_id TEXT NOT NULL,
            system TEXT NOT NULL,
            project TEXT NOT NULL,
            item_key TEXT NOT NULL,
            provider_id TEXT,
            url TEXT,
            PRIMARY KEY (target_id, brain_id),
            FOREIGN KEY(target_id) REFERENCES targets(id) ON DELETE CASCADE,
            FOREIGN KEY(target_id, brain_id) REFERENCES work_items(target_id, brain_id) ON DELETE CASCADE
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_targets_key ON targets(key)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_files_target_path ON files(target_id, path)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_nodes_target_path ON nodes(target_id, path)")
        .execute(pool)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_backlinks_target_target_path ON backlinks(target_id, target_path)",
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_edges_target_from ON edges(target_id, from_id)")
        .execute(pool)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_work_items_target_content_path ON work_items(target_id, content_path)",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Idempotent `ALTER TABLE ADD COLUMN`. SQLite does not support
/// `ADD COLUMN IF NOT EXISTS`, so we probe `pragma_table_info` first and
/// skip when the column already exists. Used by the 3.7B-α migration to
/// extend `targets` without re-creating the table.
async fn add_column_if_missing(
    pool: &SqlitePool,
    table: &str,
    column: &str,
    spec: &str,
) -> Result<(), sqlx::Error> {
    let existing: Vec<(String,)> =
        sqlx::query_as(&format!("SELECT name FROM pragma_table_info('{table}')"))
            .fetch_all(pool)
            .await?;
    if existing.iter().any(|(name,)| name == column) {
        return Ok(());
    }
    let stmt = format!("ALTER TABLE {table} ADD COLUMN {column} {spec}");
    sqlx::query(&stmt).execute(pool).await?;
    Ok(())
}

pub async fn load_work_item_by_path(
    target: &TargetConfig,
    path: &str,
) -> Result<Option<WorkItem>, BrainError> {
    read_work_item_by_path(target, path).await
}

#[derive(Clone, Debug)]
pub struct NodeFilters {
    pub node_types: Vec<String>,
    pub tags: Vec<String>,
    pub paths: Vec<String>,
    pub path_prefix: Option<String>,
    pub include_virtual: bool,
}

impl Default for NodeFilters {
    fn default() -> Self {
        Self {
            node_types: Vec::new(),
            tags: Vec::new(),
            paths: Vec::new(),
            path_prefix: None,
            include_virtual: true,
        }
    }
}

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

#[derive(Clone, Debug, Default)]
pub struct WorkItemFilters {
    pub brain_ids: Vec<String>,
    pub kinds: Vec<WorkItemKind>,
    pub states: Vec<WorkItemState>,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub content_paths: Vec<String>,
}

pub async fn list_nodes(
    target: &TargetConfig,
    filters: &NodeFilters,
) -> Result<Vec<Node>, BrainError> {
    let pool = pool()?;
    let target_id = ensure_target_id(pool, target).await?;
    list_nodes_from_pool(pool, target_id, filters).await
}

pub async fn list_files(
    target: &TargetConfig,
    filters: &FileFilters,
) -> Result<Vec<ProjectedFile>, BrainError> {
    let pool = pool()?;
    let target_id = ensure_target_id(pool, target).await?;
    list_files_from_pool(pool, target_id, filters).await
}

pub async fn read_node(target: &TargetConfig, path: &str) -> Result<Option<Node>, BrainError> {
    let mut filters = NodeFilters {
        paths: vec![path.to_string()],
        include_virtual: false,
        ..Default::default()
    };
    filters.paths.retain(|p| !p.trim().is_empty());
    Ok(list_nodes(target, &filters).await?.into_iter().next())
}

fn normalize_path_prefix(prefix: &str) -> String {
    let trimmed = prefix.trim().trim_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}/")
    }
}

pub async fn list_work_items(
    target: &TargetConfig,
    filters: &WorkItemFilters,
) -> Result<Vec<WorkItem>, BrainError> {
    let pool = pool()?;
    let target_id = ensure_target_id(pool, target).await?;
    list_work_items_from_pool(pool, target_id, filters).await
}

pub async fn read_work_item_by_path(
    target: &TargetConfig,
    path: &str,
) -> Result<Option<WorkItem>, BrainError> {
    let filters = WorkItemFilters {
        content_paths: vec![path.to_string()],
        ..Default::default()
    };
    Ok(list_work_items(target, &filters).await?.into_iter().next())
}

/// Load a single work item by its stable `brain_id`. Used by mutate-only
/// server fns that already know the identity and don't need a path lookup.
pub async fn load_work_item_by_brain_id(
    target: &TargetConfig,
    brain_id: &str,
) -> Result<Option<WorkItem>, BrainError> {
    let pool = pool()?;
    let target_id = ensure_target_id(pool, target).await?;
    let row = sqlx::query(
        "SELECT content_path FROM work_items WHERE target_id = ? AND brain_id = ? LIMIT 1",
    )
    .bind(target_id)
    .bind(brain_id)
    .fetch_optional(pool)
    .await
    .map_err(sqlx_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let path: Option<String> = row.get("content_path");
    let Some(path) = path else {
        return Ok(None);
    };
    load_work_item_by_path_from_pool(pool, target, &path).await
}

/// Update only the `state` column of a work item. Caller is responsible for
/// having already updated the markdown frontmatter on the forge — this keeps
/// the local read model in sync without a full rebuild.
pub async fn update_work_item_state(
    target: &TargetConfig,
    brain_id: &str,
    state: &WorkItemState,
) -> Result<(), BrainError> {
    let pool = pool()?;
    let target_id = ensure_target_id(pool, target).await?;
    sqlx::query("UPDATE work_items SET state = ? WHERE target_id = ? AND brain_id = ?")
        .bind(enum_str(state))
        .bind(target_id)
        .bind(brain_id)
        .execute(pool)
        .await
        .map_err(sqlx_error)?;
    Ok(())
}

/// Replace the assignees JSON column.
pub async fn update_work_item_assignees(
    target: &TargetConfig,
    brain_id: &str,
    assignees: &[String],
) -> Result<(), BrainError> {
    let pool = pool()?;
    let target_id = ensure_target_id(pool, target).await?;
    let assignees_json = serde_json::to_string(assignees)
        .map_err(|error| BrainError::parse(format!("projection assignees json: {error}")))?;
    sqlx::query("UPDATE work_items SET assignees_json = ? WHERE target_id = ? AND brain_id = ?")
        .bind(assignees_json)
        .bind(target_id)
        .bind(brain_id)
        .execute(pool)
        .await
        .map_err(sqlx_error)?;
    Ok(())
}

/// Look up the work item bound to a given external item, if any. Returns
/// the matching `(brain_id, content_path)` so the webhook handler can emit
/// a granular SSE event without doing a second round trip.
pub async fn find_work_item_by_external(
    target: &TargetConfig,
    system: &str,
    project: &str,
    item_key: &str,
) -> Result<Option<(String, Option<String>)>, BrainError> {
    let pool = pool()?;
    let target_id = ensure_target_id(pool, target).await?;
    let row = sqlx::query(
        "SELECT wi.brain_id, wi.content_path
         FROM work_item_bindings wb
         JOIN work_items wi
           ON wi.target_id = wb.target_id AND wi.brain_id = wb.brain_id
         WHERE wb.target_id = ? AND wb.system = ? AND wb.project = ? AND wb.item_key = ?
         LIMIT 1",
    )
    .bind(target_id)
    .bind(system)
    .bind(project)
    .bind(item_key)
    .fetch_optional(pool)
    .await
    .map_err(sqlx_error)?;
    Ok(row.map(|row| {
        (
            row.get::<String, _>("brain_id"),
            row.get::<Option<String>, _>("content_path"),
        )
    }))
}

/// Set or clear the external binding for a work item. Passing `None` deletes
/// the binding row; passing `Some(_)` upserts it.
pub async fn upsert_work_item_binding(
    target: &TargetConfig,
    brain_id: &str,
    binding: Option<&ExternalWorkItemBinding>,
) -> Result<(), BrainError> {
    let pool = pool()?;
    let target_id = ensure_target_id(pool, target).await?;
    match binding {
        None => {
            sqlx::query("DELETE FROM work_item_bindings WHERE target_id = ? AND brain_id = ?")
                .bind(target_id)
                .bind(brain_id)
                .execute(pool)
                .await
                .map_err(sqlx_error)?;
        }
        Some(b) => {
            sqlx::query(
                "INSERT INTO work_item_bindings (
                    target_id, brain_id, system, project, item_key, provider_id, url
                ) VALUES (?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(target_id, brain_id) DO UPDATE SET
                    system = excluded.system,
                    project = excluded.project,
                    item_key = excluded.item_key,
                    provider_id = excluded.provider_id,
                    url = excluded.url",
            )
            .bind(target_id)
            .bind(brain_id)
            .bind(enum_str(&b.system))
            .bind(&b.project)
            .bind(&b.item_key)
            .bind(&b.provider_id)
            .bind(&b.url)
            .execute(pool)
            .await
            .map_err(sqlx_error)?;
        }
    }
    Ok(())
}

pub async fn load_graph(
    storage: &GithubStorage,
    token: &str,
    config: &BrainConfig,
) -> Result<(Vec<Node>, Vec<Edge>), BrainError> {
    let pool = pool()?;
    let target = storage.target().clone();
    let target_id = ensure_target_id(pool, &target).await?;
    let state = load_sync_state(pool, target_id).await?;
    let has_success = state
        .as_ref()
        .and_then(|sync| sync.last_success_at.as_ref())
        .is_some();

    if !has_success {
        match rebuild(storage, token, config, "bootstrap").await {
            Ok(()) => {}
            Err(error) => {
                tracing::warn!(
                    target = %TargetKey::from(&target),
                    error = %error,
                    "projection bootstrap rebuild failed"
                );
                return Err(error);
            }
        }
    }

    load_cached_graph(pool, target_id).await
}

pub async fn rebuild(
    storage: &GithubStorage,
    token: &str,
    config: &BrainConfig,
    reason: &str,
) -> Result<(), BrainError> {
    let pool = pool()?;
    let target = storage.target().clone();
    let target_id = ensure_target_id(pool, &target).await?;
    record_attempt(pool, target_id, reason).await?;

    let result = async {
        let raw_files = storage.fetch_raw_files(token).await?;
        let snapshot = ProjectionSnapshot::from_raw_files(&raw_files, config);
        persist_snapshot(pool, target_id, &snapshot, reason).await
    }
    .await;

    match result {
        Ok(()) => {
            tracing::info!(
                target = %TargetKey::from(&target),
                reason,
                "projection rebuild completed"
            );
            Ok(())
        }
        Err(error) => {
            if let Err(state_error) =
                record_failure(pool, target_id, reason, &error.to_string()).await
            {
                tracing::warn!(
                    target = %TargetKey::from(&target),
                    reason,
                    error = %state_error,
                    "failed to record projection rebuild failure"
                );
            }
            Err(error)
        }
    }
}

#[derive(Clone, Debug, Default)]
struct ProjectionSnapshot {
    files: Vec<ProjectionFile>,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    backlinks: Vec<Backlink>,
    work_items: Vec<ProjectedWorkItem>,
    work_item_bindings: Vec<ProjectedWorkItemBinding>,
}

impl ProjectionSnapshot {
    fn from_raw_files(raw_files: &[RawFile], config: &BrainConfig) -> Self {
        let (nodes, edges) = build_graph(raw_files, config);
        let files = raw_files
            .iter()
            .map(|file| ProjectionFile {
                path: file.path.clone(),
                sha: file.sha.clone(),
                size_bytes: file.content.len() as i64,
            })
            .collect();

        let parsed: Vec<_> = raw_files
            .iter()
            .filter_map(|file| parse_file(&file.content, &file.path, &file.sha))
            .collect();
        let parsed_by_path: HashMap<&str, _> =
            parsed.iter().map(|doc| (doc.rel.as_str(), doc)).collect();
        let known_paths: HashSet<String> = parsed.iter().map(|doc| doc.rel.clone()).collect();
        let mut work_items = Vec::new();
        let mut work_item_bindings = Vec::new();

        for file in raw_files {
            let Some(doc) = parsed_by_path.get(file.path.as_str()) else {
                continue;
            };
            let Some(spec) = config.lookup(&doc.node_type) else {
                continue;
            };
            let Some(kind) = spec.work_item_kind.clone() else {
                continue;
            };

            let Some((item, binding)) = project_work_item(file, doc.title.as_str(), kind, config)
            else {
                continue;
            };
            work_items.push(item);
            if let Some(binding) = binding {
                work_item_bindings.push(binding);
            }
        }

        let mut seen = HashSet::<(String, String)>::new();
        let mut backlinks = Vec::new();
        for doc in parsed {
            let from_dir = Path::new(&doc.rel).parent().unwrap_or(Path::new(""));
            for link in doc.links {
                let Some(target_path) = resolve_link_path(from_dir, &link) else {
                    continue;
                };
                if target_path == doc.rel || !known_paths.contains(&target_path) {
                    continue;
                }
                let pair = (doc.rel.clone(), target_path.clone());
                if seen.insert(pair.clone()) {
                    backlinks.push(Backlink {
                        source_path: pair.0,
                        target_path: pair.1,
                    });
                }
            }
        }

        Self {
            files,
            nodes,
            edges,
            backlinks,
            work_items,
            work_item_bindings,
        }
    }
}

#[derive(Clone, Debug)]
struct ProjectionFile {
    path: String,
    sha: String,
    size_bytes: i64,
}

#[derive(Clone, Debug)]
struct Backlink {
    source_path: String,
    target_path: String,
}

#[derive(Clone, Debug)]
struct ProjectedWorkItem {
    brain_id: String,
    kind: WorkItemKind,
    title: String,
    state: WorkItemState,
    labels_json: String,
    assignees_json: String,
    content_path: Option<String>,
    system_of_record: WorkItemSystemOfRecord,
}

#[derive(Clone, Debug)]
struct ProjectedWorkItemBinding {
    brain_id: String,
    binding: ExternalWorkItemBinding,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug)]
struct SyncState {
    status: String,
    last_success_at: Option<String>,
}

async fn ensure_target_id(pool: &SqlitePool, target: &TargetConfig) -> Result<i64, BrainError> {
    let key = TargetKey::from(target);
    sqlx::query(
        "INSERT INTO targets (key, org, repo, branch, default_branch) VALUES (?, ?, ?, ?, ?)
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

    let row = sqlx::query("SELECT id FROM targets WHERE key = ?")
        .bind(key.as_str())
        .fetch_one(pool)
        .await
        .map_err(sqlx_error)?;
    Ok(row.get::<i64, _>("id"))
}

#[cfg_attr(not(test), allow(dead_code))]
async fn load_sync_state(
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

async fn record_attempt(pool: &SqlitePool, target_id: i64, reason: &str) -> Result<(), BrainError> {
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

async fn record_failure(
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

async fn persist_snapshot(
    pool: &SqlitePool,
    target_id: i64,
    snapshot: &ProjectionSnapshot,
    reason: &str,
) -> Result<(), BrainError> {
    let mut tx = pool.begin().await.map_err(sqlx_error)?;

    sqlx::query("DELETE FROM work_item_bindings WHERE target_id = ?")
        .bind(target_id)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_error)?;
    sqlx::query("DELETE FROM work_items WHERE target_id = ?")
        .bind(target_id)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_error)?;
    sqlx::query("DELETE FROM backlinks WHERE target_id = ?")
        .bind(target_id)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_error)?;
    sqlx::query("DELETE FROM edges WHERE target_id = ?")
        .bind(target_id)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_error)?;
    sqlx::query("DELETE FROM nodes WHERE target_id = ?")
        .bind(target_id)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_error)?;
    sqlx::query("DELETE FROM files WHERE target_id = ?")
        .bind(target_id)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_error)?;

    bulk_insert_files(&mut tx, target_id, &snapshot.files).await?;
    bulk_insert_nodes(&mut tx, target_id, &snapshot.nodes).await?;
    bulk_insert_edges(&mut tx, target_id, &snapshot.edges).await?;
    bulk_insert_backlinks(&mut tx, target_id, &snapshot.backlinks).await?;
    bulk_insert_work_items(&mut tx, target_id, &snapshot.work_items).await?;
    bulk_insert_work_item_bindings(&mut tx, target_id, &snapshot.work_item_bindings).await?;

    sqlx::query(
        "INSERT INTO projection_sync_state (
            target_id, status, last_attempt_at, last_success_at, last_error_at, last_error, last_reason, file_count, node_count, edge_count
        ) VALUES (?, 'ready', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, NULL, NULL, ?, ?, ?, ?)
        ON CONFLICT(target_id) DO UPDATE SET
            status = 'ready',
            last_attempt_at = CURRENT_TIMESTAMP,
            last_success_at = CURRENT_TIMESTAMP,
            last_error_at = NULL,
            last_error = NULL,
            last_reason = excluded.last_reason,
            file_count = excluded.file_count,
            node_count = excluded.node_count,
            edge_count = excluded.edge_count",
    )
    .bind(target_id)
    .bind(reason)
    .bind(snapshot.files.len() as i64)
    .bind(snapshot.nodes.len() as i64)
    .bind(snapshot.edges.len() as i64)
    .execute(&mut *tx)
    .await
    .map_err(sqlx_error)?;

    tx.commit().await.map_err(sqlx_error)?;
    Ok(())
}

async fn bulk_insert_files(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    target_id: i64,
    files: &[ProjectionFile],
) -> Result<(), BrainError> {
    if files.is_empty() {
        return Ok(());
    }

    for chunk in files.chunks(max_rows_per_insert(4)) {
        let mut query = QueryBuilder::<Sqlite>::new(
            "INSERT INTO files (target_id, path, sha, size_bytes, updated_at) ",
        );
        query.push_values(chunk, |mut row, file| {
            row.push_bind(target_id)
                .push_bind(&file.path)
                .push_bind(&file.sha)
                .push_bind(file.size_bytes)
                .push("CURRENT_TIMESTAMP");
        });
        query.build().execute(&mut **tx).await.map_err(sqlx_error)?;
    }

    Ok(())
}

async fn bulk_insert_nodes(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    target_id: i64,
    nodes: &[Node],
) -> Result<(), BrainError> {
    if nodes.is_empty() {
        return Ok(());
    }

    let rows = nodes
        .iter()
        .map(|node| {
            Ok(NodeInsertRow {
                node_id: i64::from(node.id),
                title: node.title.clone(),
                summary: node.summary.clone(),
                node_type: node.node_type.clone(),
                tags_json: serde_json::to_string(&node.tags)
                    .map_err(|error| BrainError::parse(format!("projection tags json: {error}")))?,
                x: node.x as f64,
                y: node.y as f64,
                path: node.path.clone(),
                sha: node.sha.clone(),
                is_virtual: node.path.is_empty(),
            })
        })
        .collect::<Result<Vec<_>, BrainError>>()?;

    for chunk in rows.chunks(max_rows_per_insert(11)) {
        let mut query = QueryBuilder::<Sqlite>::new(
            "INSERT INTO nodes (
                target_id, node_id, title, summary, node_type, tags_json, x, y, path, sha, is_virtual
            ) ",
        );
        query.push_values(chunk, |mut row, node| {
            row.push_bind(target_id)
                .push_bind(node.node_id)
                .push_bind(&node.title)
                .push_bind(&node.summary)
                .push_bind(&node.node_type)
                .push_bind(&node.tags_json)
                .push_bind(node.x)
                .push_bind(node.y)
                .push_bind(&node.path)
                .push_bind(&node.sha)
                .push_bind(node.is_virtual);
        });
        query.build().execute(&mut **tx).await.map_err(sqlx_error)?;
    }

    Ok(())
}

async fn bulk_insert_edges(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    target_id: i64,
    edges: &[Edge],
) -> Result<(), BrainError> {
    if edges.is_empty() {
        return Ok(());
    }

    for chunk in edges.chunks(max_rows_per_insert(3)) {
        let mut query =
            QueryBuilder::<Sqlite>::new("INSERT INTO edges (target_id, from_id, to_id) ");
        query.push_values(chunk, |mut row, edge| {
            row.push_bind(target_id)
                .push_bind(i64::from(edge.from))
                .push_bind(i64::from(edge.to));
        });
        query.build().execute(&mut **tx).await.map_err(sqlx_error)?;
    }

    Ok(())
}

async fn bulk_insert_backlinks(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    target_id: i64,
    backlinks: &[Backlink],
) -> Result<(), BrainError> {
    if backlinks.is_empty() {
        return Ok(());
    }

    for chunk in backlinks.chunks(max_rows_per_insert(3)) {
        let mut query = QueryBuilder::<Sqlite>::new(
            "INSERT INTO backlinks (target_id, source_path, target_path) ",
        );
        query.push_values(chunk, |mut row, backlink| {
            row.push_bind(target_id)
                .push_bind(&backlink.source_path)
                .push_bind(&backlink.target_path);
        });
        query.build().execute(&mut **tx).await.map_err(sqlx_error)?;
    }

    Ok(())
}

async fn bulk_insert_work_items(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    target_id: i64,
    work_items: &[ProjectedWorkItem],
) -> Result<(), BrainError> {
    if work_items.is_empty() {
        return Ok(());
    }

    for chunk in work_items.chunks(max_rows_per_insert(8)) {
        let mut query = QueryBuilder::<Sqlite>::new(
            "INSERT INTO work_items (
                target_id, brain_id, kind, title, state, labels_json, assignees_json, content_path, system_of_record
            ) ",
        );
        query.push_values(chunk, |mut row, item| {
            row.push_bind(target_id)
                .push_bind(&item.brain_id)
                .push_bind(enum_str(&item.kind))
                .push_bind(&item.title)
                .push_bind(enum_str(&item.state))
                .push_bind(&item.labels_json)
                .push_bind(&item.assignees_json)
                .push_bind(&item.content_path)
                .push_bind(enum_str(&item.system_of_record));
        });
        query.build().execute(&mut **tx).await.map_err(sqlx_error)?;
    }

    Ok(())
}

async fn bulk_insert_work_item_bindings(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    target_id: i64,
    bindings: &[ProjectedWorkItemBinding],
) -> Result<(), BrainError> {
    if bindings.is_empty() {
        return Ok(());
    }

    for chunk in bindings.chunks(max_rows_per_insert(7)) {
        let mut query = QueryBuilder::<Sqlite>::new(
            "INSERT INTO work_item_bindings (
                target_id, brain_id, system, project, item_key, provider_id, url
            ) ",
        );
        query.push_values(chunk, |mut row, item| {
            row.push_bind(target_id)
                .push_bind(&item.brain_id)
                .push_bind(enum_str(&item.binding.system))
                .push_bind(&item.binding.project)
                .push_bind(&item.binding.item_key)
                .push_bind(&item.binding.provider_id)
                .push_bind(&item.binding.url);
        });
        query.build().execute(&mut **tx).await.map_err(sqlx_error)?;
    }

    Ok(())
}

fn max_rows_per_insert(bind_count_per_row: usize) -> usize {
    (SQLITE_MAX_VARIABLES / bind_count_per_row).max(1)
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

async fn load_work_item_by_path_from_pool(
    pool: &SqlitePool,
    target: &TargetConfig,
    path: &str,
) -> Result<Option<WorkItem>, BrainError> {
    let target_id = ensure_target_id(pool, target).await?;
    let filters = WorkItemFilters {
        content_paths: vec![path.to_string()],
        ..Default::default()
    };
    Ok(list_work_items_from_pool(pool, target_id, &filters)
        .await?
        .into_iter()
        .next())
}

async fn list_nodes_from_pool(
    pool: &SqlitePool,
    target_id: i64,
    filters: &NodeFilters,
) -> Result<Vec<Node>, BrainError> {
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT node_id, title, summary, node_type, tags_json, x, y, path, sha
         FROM nodes WHERE target_id = ",
    );
    query.push_bind(target_id);

    if !filters.include_virtual {
        query.push(" AND is_virtual = 0");
    }
    if !filters.node_types.is_empty() {
        query.push(" AND node_type IN (");
        let mut separated = query.separated(", ");
        for node_type in &filters.node_types {
            separated.push_bind(node_type);
        }
        separated.push_unseparated(")");
    }
    if !filters.paths.is_empty() {
        query.push(" AND path IN (");
        let mut separated = query.separated(", ");
        for path in &filters.paths {
            separated.push_bind(path);
        }
        separated.push_unseparated(")");
    }
    if let Some(prefix) = filters
        .path_prefix
        .as_deref()
        .map(normalize_path_prefix)
        .filter(|p| !p.is_empty())
    {
        query.push(" AND path LIKE ");
        query.push_bind(format!("{prefix}%"));
    }
    query.push(" ORDER BY node_id ASC");

    let wanted_tags: HashSet<String> = filters.tags.iter().map(|t| t.to_lowercase()).collect();
    let rows = query.build().fetch_all(pool).await.map_err(sqlx_error)?;
    let mut nodes = Vec::with_capacity(rows.len());
    for row in rows {
        let node = node_from_row(row)?;
        if !wanted_tags.is_empty()
            && !node
                .tags
                .iter()
                .any(|tag| wanted_tags.contains(&tag.to_lowercase()))
        {
            continue;
        }
        nodes.push(node);
    }
    Ok(nodes)
}

async fn list_files_from_pool(
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

async fn list_edges_from_pool(pool: &SqlitePool, target_id: i64) -> Result<Vec<Edge>, BrainError> {
    let edge_rows = sqlx::query(
        "SELECT from_id, to_id FROM edges WHERE target_id = ? ORDER BY from_id ASC, to_id ASC",
    )
    .bind(target_id)
    .fetch_all(pool)
    .await
    .map_err(sqlx_error)?;
    Ok(edge_rows
        .into_iter()
        .map(|row| Edge {
            from: row.get::<i64, _>("from_id") as u32,
            to: row.get::<i64, _>("to_id") as u32,
        })
        .collect())
}

async fn list_work_items_from_pool(
    pool: &SqlitePool,
    target_id: i64,
    filters: &WorkItemFilters,
) -> Result<Vec<WorkItem>, BrainError> {
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT
            wi.brain_id,
            wi.kind,
            wi.title,
            wi.state,
            wi.labels_json,
            wi.assignees_json,
            wi.content_path,
            wi.system_of_record,
            wb.system AS binding_system,
            wb.project AS binding_project,
            wb.item_key AS binding_item_key,
            wb.provider_id AS binding_provider_id,
            wb.url AS binding_url
        FROM work_items wi
        LEFT JOIN work_item_bindings wb
            ON wb.target_id = wi.target_id
           AND wb.brain_id = wi.brain_id
        WHERE wi.target_id = ",
    );
    query.push_bind(target_id);

    if !filters.brain_ids.is_empty() {
        query.push(" AND wi.brain_id IN (");
        let mut separated = query.separated(", ");
        for brain_id in &filters.brain_ids {
            separated.push_bind(brain_id);
        }
        separated.push_unseparated(")");
    }
    if !filters.kinds.is_empty() {
        query.push(" AND wi.kind IN (");
        let mut separated = query.separated(", ");
        for kind in &filters.kinds {
            separated.push_bind(enum_str(kind));
        }
        separated.push_unseparated(")");
    }
    if !filters.states.is_empty() {
        query.push(" AND wi.state IN (");
        let mut separated = query.separated(", ");
        for state in &filters.states {
            separated.push_bind(enum_str(state));
        }
        separated.push_unseparated(")");
    }
    if !filters.content_paths.is_empty() {
        query.push(" AND wi.content_path IN (");
        let mut separated = query.separated(", ");
        for path in &filters.content_paths {
            separated.push_bind(path);
        }
        separated.push_unseparated(")");
    }
    query.push(" ORDER BY wi.title ASC, wi.brain_id ASC");

    let wanted_labels: HashSet<String> = filters
        .labels
        .iter()
        .map(|label| label.to_lowercase())
        .collect();
    let wanted_assignees: HashSet<String> = filters
        .assignees
        .iter()
        .map(|assignee| assignee.to_lowercase())
        .collect();
    let rows = query.build().fetch_all(pool).await.map_err(sqlx_error)?;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let item = work_item_from_row(row)?;
        if !wanted_labels.is_empty()
            && !item
                .labels
                .iter()
                .any(|label| wanted_labels.contains(&label.to_lowercase()))
        {
            continue;
        }
        if !wanted_assignees.is_empty()
            && !item
                .assignees
                .iter()
                .any(|assignee| wanted_assignees.contains(&assignee.to_lowercase()))
        {
            continue;
        }
        items.push(item);
    }
    Ok(items)
}

fn node_from_row(row: SqliteRow) -> Result<Node, BrainError> {
    let tags_json = row.get::<String, _>("tags_json");
    let tags = serde_json::from_str::<Vec<String>>(&tags_json)
        .map_err(|error| BrainError::parse(format!("projection tags parse: {error}")))?;
    Ok(Node {
        id: row.get::<i64, _>("node_id") as u32,
        title: row.get::<String, _>("title"),
        summary: row.get::<String, _>("summary"),
        node_type: row.get::<String, _>("node_type"),
        tags,
        x: row.get::<f64, _>("x") as f32,
        y: row.get::<f64, _>("y") as f32,
        path: row.get::<String, _>("path"),
        sha: row.get::<String, _>("sha"),
    })
}

fn work_item_from_row(row: SqliteRow) -> Result<WorkItem, BrainError> {
    let labels = serde_json::from_str(&row.get::<String, _>("labels_json"))
        .map_err(|error| BrainError::parse(format!("projection labels parse: {error}")))?;
    let assignees = serde_json::from_str(&row.get::<String, _>("assignees_json"))
        .map_err(|error| BrainError::parse(format!("projection assignees parse: {error}")))?;
    let external_binding = match row.get::<Option<String>, _>("binding_system") {
        Some(system) => Some(ExternalWorkItemBinding {
            system: parse_enum_str(&system)?,
            project: row.get::<String, _>("binding_project"),
            item_key: row.get::<String, _>("binding_item_key"),
            provider_id: row.get::<Option<String>, _>("binding_provider_id"),
            url: row.get::<Option<String>, _>("binding_url"),
        }),
        None => None,
    };

    Ok(WorkItem {
        brain_id: row.get::<String, _>("brain_id"),
        kind: parse_enum_str(&row.get::<String, _>("kind"))?,
        title: row.get::<String, _>("title"),
        state: parse_enum_str(&row.get::<String, _>("state"))?,
        labels,
        assignees,
        content_path: row.get::<Option<String>, _>("content_path"),
        external_binding,
        system_of_record: parse_enum_str(&row.get::<String, _>("system_of_record"))?,
    })
}

fn project_work_item(
    file: &RawFile,
    title: &str,
    kind: WorkItemKind,
    config: &BrainConfig,
) -> Option<(ProjectedWorkItem, Option<ProjectedWorkItemBinding>)> {
    let (frontmatter, _body) = brain_domain::split_frontmatter(&file.content);
    if frontmatter.trim().is_empty() {
        return None;
    }
    let map = serde_yaml::from_str::<BTreeMap<String, Value>>(frontmatter)
        .unwrap_or_else(|_| BTreeMap::new());

    let brain_id = string_field(&map, "brain_id").unwrap_or_else(|| file.path.clone());
    let state = enum_field::<WorkItemState>(&map, "state").unwrap_or(WorkItemState::Todo);
    let assignees = string_list_field(&map, "assignees");
    let binding = enum_object_field::<ExternalWorkItemBinding>(&map, "external_binding");
    let system_of_record = enum_field::<WorkItemSystemOfRecord>(&map, "system_of_record")
        .unwrap_or_else(|| {
            if binding.is_some() {
                WorkItemSystemOfRecord::Split
            } else {
                WorkItemSystemOfRecord::Brain
            }
        });

    let mut labels = Vec::new();
    if let Some(label_spec) = config.labels_for_kind(&kind) {
        labels.push(label_spec.kind_label.clone());
        if let Some(label) = label_spec.state_labels.get(&state) {
            labels.push(label.clone());
        }
    }

    let labels_json = serde_json::to_string(&labels)
        .map_err(|error| BrainError::parse(format!("projection labels json: {error}")))
        .ok()?;
    let assignees_json = serde_json::to_string(&assignees)
        .map_err(|error| BrainError::parse(format!("projection assignees json: {error}")))
        .ok()?;

    Some((
        ProjectedWorkItem {
            brain_id: brain_id.clone(),
            kind,
            title: title.to_string(),
            state,
            labels_json,
            assignees_json,
            content_path: Some(file.path.clone()),
            system_of_record,
        },
        binding.map(|binding| ProjectedWorkItemBinding { brain_id, binding }),
    ))
}

fn string_field(map: &BTreeMap<String, Value>, key: &str) -> Option<String> {
    map.get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn string_list_field(map: &BTreeMap<String, Value>, key: &str) -> Vec<String> {
    map.get(key)
        .and_then(|value| value.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn enum_field<T>(map: &BTreeMap<String, Value>, key: &str) -> Option<T>
where
    T: serde::de::DeserializeOwned,
{
    map.get(key)
        .cloned()
        .and_then(|value| serde_yaml::from_value(value).ok())
}

fn enum_object_field<T>(map: &BTreeMap<String, Value>, key: &str) -> Option<T>
where
    T: serde::de::DeserializeOwned,
{
    map.get(key)
        .cloned()
        .and_then(|value| serde_yaml::from_value(value).ok())
}

#[derive(Clone, Debug)]
struct NodeInsertRow {
    node_id: i64,
    title: String,
    summary: String,
    node_type: String,
    tags_json: String,
    x: f64,
    y: f64,
    path: String,
    sha: String,
    is_virtual: bool,
}

async fn load_cached_graph(
    pool: &SqlitePool,
    target_id: i64,
) -> Result<(Vec<Node>, Vec<Edge>), BrainError> {
    let nodes = list_nodes_from_pool(pool, target_id, &NodeFilters::default()).await?;
    let edges = list_edges_from_pool(pool, target_id).await?;
    Ok((nodes, edges))
}

fn resolve_link_path(from_dir: &Path, link: &str) -> Option<String> {
    let joined = from_dir.join(link);
    let mut parts: Vec<&str> = Vec::new();
    for component in joined.iter() {
        let segment = component.to_str()?;
        if segment == "." {
            continue;
        }
        if segment == ".." {
            parts.pop();
            continue;
        }
        parts.push(segment);
    }
    Some(parts.join("/"))
}

fn sqlx_error(error: sqlx::Error) -> BrainError {
    BrainError::Io(format!("projection sqlite: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;

    fn target(org: &str, repo: &str, branch: &str) -> TargetConfig {
        TargetConfig {
            org: org.to_string(),
            repo: repo.to_string(),
            branch: branch.to_string(),
        }
    }

    fn raw(path: &str, sha: &str, content: &str) -> RawFile {
        RawFile {
            path: path.to_string(),
            sha: sha.to_string(),
            content: content.to_string(),
        }
    }

    async fn test_pool() -> SqlitePool {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .unwrap();
        migrate(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn snapshot_persists_per_target_without_cross_talk() {
        let pool = test_pool().await;
        let config = BrainConfig::default();

        let snapshot_a = ProjectionSnapshot::from_raw_files(
            &[raw(
                "concepts/A.md",
                "sha-a",
                "---\ntype: concept\ntopic: A\n---\nsee [B](./B.md)\n",
            )],
            &config,
        );
        let snapshot_b = ProjectionSnapshot::from_raw_files(
            &[raw(
                "concepts/B.md",
                "sha-b",
                "---\ntype: concept\ntopic: B\n---\nbody\n",
            )],
            &config,
        );

        let target_a = target("org", "repo-a", "main");
        let target_b = target("org", "repo-b", "main");
        let target_a_id = ensure_target_id(&pool, &target_a).await.unwrap();
        let target_b_id = ensure_target_id(&pool, &target_b).await.unwrap();

        persist_snapshot(&pool, target_a_id, &snapshot_a, "test-a")
            .await
            .unwrap();
        persist_snapshot(&pool, target_b_id, &snapshot_b, "test-b")
            .await
            .unwrap();

        let (nodes_a, _) = load_cached_graph(&pool, target_a_id).await.unwrap();
        let (nodes_b, _) = load_cached_graph(&pool, target_b_id).await.unwrap();

        assert_eq!(nodes_a.len(), 1);
        assert_eq!(nodes_b.len(), 1);
        assert_eq!(nodes_a[0].title, "A");
        assert_eq!(nodes_b[0].title, "B");
    }

    #[tokio::test]
    async fn snapshot_materializes_work_items_and_bindings() {
        let pool = test_pool().await;
        let config = BrainConfig::parse(
            r##"
default_type: task
node_types:
  - name: task
    label: Task
    directory: tasks
    accent: "#fb7185"
    title_key: topic
    work_item_kind: task
"##,
        )
        .unwrap();

        let snapshot = ProjectionSnapshot::from_raw_files(
            &[raw(
                "tasks/stabilize-sync.md",
                "sha-task",
                "---\ntype: task\ntopic: Stabilize sync\nbrain_id: task-sync-1\nstate: in-progress\nassignees: [alice, bob]\nexternal_binding:\n  system: github\n  project: AndreaBozzo/Brain_UI\n  item_key: \"42\"\n  url: https://github.com/AndreaBozzo/Brain_UI/issues/42\n---\n# Task: Stabilize sync\n\n## Description\nBody\n",
            )],
            &config,
        );

        let target_id = ensure_target_id(&pool, &target("org", "repo-workitems", "main"))
            .await
            .unwrap();
        persist_snapshot(&pool, target_id, &snapshot, "test-work-items")
            .await
            .unwrap();

        let item = sqlx::query(
                "SELECT brain_id, kind, title, state, labels_json, assignees_json, content_path, system_of_record FROM work_items WHERE target_id = ?",
            )
            .bind(target_id)
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(item.get::<String, _>("brain_id"), "task-sync-1");
        assert_eq!(item.get::<String, _>("kind"), "task");
        assert_eq!(item.get::<String, _>("title"), "Stabilize Sync");
        assert_eq!(item.get::<String, _>("state"), "in-progress");
        assert_eq!(
            item.get::<String, _>("content_path"),
            "tasks/stabilize-sync.md"
        );
        assert_eq!(item.get::<String, _>("system_of_record"), "split");
        assert_eq!(
            serde_json::from_str::<Vec<String>>(&item.get::<String, _>("labels_json")).unwrap(),
            vec!["brain:task".to_string(), "brain:in-progress".to_string()]
        );
        assert_eq!(
            serde_json::from_str::<Vec<String>>(&item.get::<String, _>("assignees_json")).unwrap(),
            vec!["alice".to_string(), "bob".to_string()]
        );

        let binding = sqlx::query(
                "SELECT system, project, item_key, url FROM work_item_bindings WHERE target_id = ? AND brain_id = ?",
            )
            .bind(target_id)
            .bind("task-sync-1")
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(binding.get::<String, _>("system"), "github");
        assert_eq!(binding.get::<String, _>("project"), "AndreaBozzo/Brain_UI");
        assert_eq!(binding.get::<String, _>("item_key"), "42");
        assert_eq!(
            binding.get::<String, _>("url"),
            "https://github.com/AndreaBozzo/Brain_UI/issues/42"
        );
    }

    #[tokio::test]
    async fn load_work_item_by_path_reads_projected_binding() {
        let pool = test_pool().await;
        let config = BrainConfig::parse(
            r##"
    default_type: task
    node_types:
      - name: task
        label: Task
        directory: tasks
        accent: "#fb7185"
        title_key: topic
        work_item_kind: task
    "##,
        )
        .unwrap();

        let snapshot = ProjectionSnapshot::from_raw_files(
            &[raw(
                "tasks/api-read.md",
                "sha-task",
                "---\ntype: task\ntopic: API read\nbrain_id: task-api-1\nstate: done\nassignees: [andrea]\nexternal_binding:\n  system: github\n  project: AndreaBozzo/Brain_UI\n  item_key: \"77\"\n  provider_id: I_kwDO123\n  url: https://github.com/AndreaBozzo/Brain_UI/issues/77\nsystem_of_record: split\n---\n# Task: API read\n",
            )],
            &config,
        );
        let target = target("org", "repo-workitems-read", "main");
        let target_id = ensure_target_id(&pool, &target).await.unwrap();
        persist_snapshot(&pool, target_id, &snapshot, "test-work-item-read")
            .await
            .unwrap();

        let item = load_work_item_by_path_from_pool(&pool, &target, "tasks/api-read.md")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(item.brain_id, "task-api-1");
        assert_eq!(item.state, WorkItemState::Done);
        assert_eq!(item.assignees, vec!["andrea".to_string()]);
        assert_eq!(item.system_of_record, WorkItemSystemOfRecord::Split);
        assert!(item.labels.contains(&"brain:task".to_string()));
        let binding = item.external_binding.expect("binding must exist");
        assert_eq!(binding.item_key, "77");
        assert_eq!(binding.project, "AndreaBozzo/Brain_UI");
    }

    #[tokio::test]
    async fn snapshot_materializes_backlinks_and_sync_state() {
        let pool = test_pool().await;
        let config = BrainConfig::default();
        let target = target("org", "repo", "main");
        let target_id = ensure_target_id(&pool, &target).await.unwrap();
        let snapshot = ProjectionSnapshot::from_raw_files(
            &[
                raw(
                    "concepts/A.md",
                    "sha-a",
                    "---\ntype: concept\ntopic: A\n---\nsee [B](./B.md)\n",
                ),
                raw(
                    "concepts/B.md",
                    "sha-b",
                    "---\ntype: concept\ntopic: B\n---\nbody\n",
                ),
            ],
            &config,
        );

        persist_snapshot(&pool, target_id, &snapshot, "bootstrap")
            .await
            .unwrap();

        let sync = load_sync_state(&pool, target_id).await.unwrap().unwrap();
        let backlink_count =
            sqlx::query("SELECT COUNT(*) AS count FROM backlinks WHERE target_id = ?")
                .bind(target_id)
                .fetch_one(&pool)
                .await
                .unwrap()
                .get::<i64, _>("count");

        assert_eq!(sync.status, "ready");
        assert!(sync.last_success_at.is_some());
        assert_eq!(backlink_count, 1);
    }

    #[tokio::test]
    async fn list_nodes_filters_projection_without_rebuild() {
        let pool = test_pool().await;
        let config = BrainConfig::default();
        let target = target("org", "repo-node-query", "main");
        let target_id = ensure_target_id(&pool, &target).await.unwrap();
        let snapshot = ProjectionSnapshot::from_raw_files(
            &[
                raw(
                    "concepts/A.md",
                    "sha-a",
                    "---\ntype: concept\ntopic: Alpha\ntags: [sync]\n---\nsee [B](./B.md)\n",
                ),
                raw(
                    "concepts/B.md",
                    "sha-b",
                    "---\ntype: concept\ntopic: Beta\ntags: [sync]\n---\nbody\n",
                ),
            ],
            &config,
        );
        persist_snapshot(&pool, target_id, &snapshot, "test-node-query")
            .await
            .unwrap();

        let non_virtual = list_nodes_from_pool(
            &pool,
            target_id,
            &NodeFilters {
                include_virtual: false,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let tagged = list_nodes_from_pool(
            &pool,
            target_id,
            &NodeFilters {
                tags: vec!["SYNC".to_string()],
                include_virtual: false,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let beta = list_nodes_from_pool(
            &pool,
            target_id,
            &NodeFilters {
                paths: vec!["concepts/B.md".to_string()],
                include_virtual: false,
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
        let prefixed = list_nodes_from_pool(
            &pool,
            target_id,
            &NodeFilters {
                path_prefix: Some("concepts".to_string()),
                include_virtual: false,
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(non_virtual.len(), 2);
        assert_eq!(tagged.len(), 2);
        assert_eq!(beta.title, "Beta");
        assert_eq!(prefixed.len(), 2);
    }

    #[tokio::test]
    async fn list_files_reports_structure_metadata() {
        let pool = test_pool().await;
        let config = BrainConfig::default();
        let target = target("org", "repo-file-query", "main");
        let target_id = ensure_target_id(&pool, &target).await.unwrap();
        let snapshot = ProjectionSnapshot::from_raw_files(
            &[
                raw(
                    "concepts/A.md",
                    "sha-a",
                    "---\ntype: concept\ntopic: Alpha\n---\nsee [B](./B.md)\n",
                ),
                raw(
                    "concepts/B.md",
                    "sha-b",
                    "---\ntype: concept\ntopic: Beta\n---\nbody\n",
                ),
                raw(
                    "runbooks/solo.md",
                    "sha-c",
                    "---\ntype: runbook\ntopic: Solo\n---\nbody\n",
                ),
            ],
            &config,
        );
        persist_snapshot(&pool, target_id, &snapshot, "test-file-query")
            .await
            .unwrap();

        let concepts = list_files_from_pool(
            &pool,
            target_id,
            &FileFilters {
                path_prefix: Some("concepts".to_string()),
                orphan_only: false,
            },
        )
        .await
        .unwrap();
        let orphan = list_files_from_pool(
            &pool,
            target_id,
            &FileFilters {
                path_prefix: None,
                orphan_only: true,
            },
        )
        .await
        .unwrap();

        assert_eq!(concepts.len(), 2);
        assert_eq!(concepts[0].title.as_deref(), Some("Alpha"));
        assert!(!concepts[0].is_orphan_in_graph);
        assert_eq!(orphan.len(), 1);
        assert_eq!(orphan[0].path, "runbooks/solo.md");
        assert!(orphan[0].is_orphan_in_graph);
    }

    #[tokio::test]
    async fn list_work_items_filters_projection_rows() {
        let pool = test_pool().await;
        let config = BrainConfig::parse(
            r##"
default_type: task
node_types:
  - name: task
    label: Task
    directory: tasks
    accent: "#fb7185"
    title_key: topic
    work_item_kind: task
"##,
        )
        .unwrap();

        let target = target("org", "repo-workitem-query", "main");
        let target_id = ensure_target_id(&pool, &target).await.unwrap();
        let snapshot = ProjectionSnapshot::from_raw_files(
            &[
                raw(
                    "tasks/a.md",
                    "sha-a",
                    "---\ntype: task\ntopic: A\nbrain_id: task-a\nstate: blocked\nassignees: [andrea]\n---\n",
                ),
                raw(
                    "tasks/b.md",
                    "sha-b",
                    "---\ntype: task\ntopic: B\nbrain_id: task-b\nstate: done\nassignees: [sam]\n---\n",
                ),
            ],
            &config,
        );
        persist_snapshot(&pool, target_id, &snapshot, "test-workitem-query")
            .await
            .unwrap();

        let blocked = list_work_items_from_pool(
            &pool,
            target_id,
            &WorkItemFilters {
                states: vec![WorkItemState::Blocked],
                assignees: vec!["ANDREA".to_string()],
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].brain_id, "task-a");
    }

    // ----- 3.7B-α migration -----

    #[tokio::test]
    async fn migrate_adds_registration_columns_to_targets() {
        let pool = test_pool().await;
        let cols: Vec<(String,)> = sqlx::query_as("SELECT name FROM pragma_table_info('targets')")
            .fetch_all(&pool)
            .await
            .unwrap();
        let names: Vec<&str> = cols.iter().map(|(n,)| n.as_str()).collect();
        for required in ["registered_at", "registered_by", "source", "default_branch"] {
            assert!(
                names.contains(&required),
                "targets is missing column {required}; got {names:?}"
            );
        }
    }

    #[tokio::test]
    async fn migrate_creates_unique_index_on_org_repo() {
        let pool = test_pool().await;
        // The UNIQUE index must reject a second row with the same (org, repo)
        // even when the branch differs — this is the stickiness invariant.
        let _ = ensure_target_id(&pool, &target("acme", "kb", "main"))
            .await
            .unwrap();
        let res = sqlx::query("INSERT INTO targets (key, org, repo, branch) VALUES (?, ?, ?, ?)")
            .bind("acme/kb/develop")
            .bind("acme")
            .bind("kb")
            .bind("develop")
            .execute(&pool)
            .await;
        assert!(res.is_err(), "expected UNIQUE(org, repo) violation, got Ok");
    }

    #[tokio::test]
    async fn migrate_is_idempotent() {
        // Running migrate twice on the same pool must not fail. ALTER TABLE
        // ADD COLUMN would error on the second run without the
        // `add_column_if_missing` guard.
        let pool = test_pool().await;
        migrate(&pool).await.expect("second migrate must succeed");
        migrate(&pool).await.expect("third migrate must succeed");
    }

    #[tokio::test]
    async fn ensure_target_id_seeds_registration_metadata() {
        // Default seed for rows created via the existing ensure_target_id
        // path (which doesn't yet know about the new columns) must be the
        // schema default `'env_default'` and CURRENT_TIMESTAMP.
        let pool = test_pool().await;
        let _id = ensure_target_id(&pool, &target("acme", "kb", "main"))
            .await
            .unwrap();
        let row: (String, Option<String>, String, Option<String>) = sqlx::query_as(
            "SELECT source, registered_by, registered_at, default_branch FROM targets
             WHERE org = ? AND repo = ?",
        )
        .bind("acme")
        .bind("kb")
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0, "env_default");
        assert!(row.1.is_none());
        assert!(!row.2.is_empty());
        assert_eq!(row.3.as_deref(), Some("main"));
    }
}
