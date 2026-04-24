use std::collections::HashSet;
use std::path::Path;
use std::sync::OnceLock;

use brain_domain::{BrainConfig, BrainError, Edge, Node, TargetConfig, TargetKey};
use brain_graph::{RawFile, build_graph, parse_file};
use brain_storage::GithubStorage;
use sqlx::{Row, SqlitePool};

static POOL: OnceLock<SqlitePool> = OnceLock::new();

pub fn init(pool: SqlitePool) {
    let _ = POOL.set(pool);
}

fn pool() -> Result<&'static SqlitePool, BrainError> {
    POOL.get()
        .ok_or_else(|| BrainError::other("Projection SQLite pool not initialized"))
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
    let status = state
        .as_ref()
        .map(|sync| sync.status.as_str())
        .unwrap_or("missing");
    let should_rebuild = !matches!(status, "ready") || !has_success;
    if should_rebuild {
        let reason = if has_success {
            "reconcile"
        } else {
            "bootstrap"
        };
        if let Err(error) = rebuild(storage, token, config, reason).await {
            if has_success {
                tracing::warn!(
                    target = %TargetKey::from(&target),
                    reason,
                    error = %error,
                    "projection rebuild failed; serving last good snapshot"
                );
                return load_cached_graph(pool, target_id).await;
            }
            return Err(error);
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
        let known_paths: HashSet<String> = parsed.iter().map(|doc| doc.rel.clone()).collect();

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
struct SyncState {
    status: String,
    last_success_at: Option<String>,
}

async fn ensure_target_id(pool: &SqlitePool, target: &TargetConfig) -> Result<i64, BrainError> {
    let key = TargetKey::from(target);
    sqlx::query(
        "INSERT INTO targets (key, org, repo, branch) VALUES (?, ?, ?, ?)
         ON CONFLICT(key) DO UPDATE SET org = excluded.org, repo = excluded.repo, branch = excluded.branch",
    )
    .bind(key.as_str())
    .bind(&target.org)
    .bind(&target.repo)
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

    for file in &snapshot.files {
        sqlx::query(
            "INSERT INTO files (target_id, path, sha, size_bytes, updated_at)
             VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP)",
        )
        .bind(target_id)
        .bind(&file.path)
        .bind(&file.sha)
        .bind(file.size_bytes)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_error)?;
    }

    for node in &snapshot.nodes {
        let tags_json = serde_json::to_string(&node.tags)
            .map_err(|error| BrainError::parse(format!("projection tags json: {error}")))?;
        sqlx::query(
            "INSERT INTO nodes (
                target_id, node_id, title, summary, node_type, tags_json, x, y, path, sha, is_virtual
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(target_id)
        .bind(i64::from(node.id))
        .bind(&node.title)
        .bind(&node.summary)
        .bind(&node.node_type)
        .bind(tags_json)
        .bind(node.x as f64)
        .bind(node.y as f64)
        .bind(&node.path)
        .bind(&node.sha)
        .bind(node.path.is_empty())
        .execute(&mut *tx)
        .await
        .map_err(sqlx_error)?;
    }

    for edge in &snapshot.edges {
        sqlx::query("INSERT INTO edges (target_id, from_id, to_id) VALUES (?, ?, ?)")
            .bind(target_id)
            .bind(i64::from(edge.from))
            .bind(i64::from(edge.to))
            .execute(&mut *tx)
            .await
            .map_err(sqlx_error)?;
    }

    for backlink in &snapshot.backlinks {
        sqlx::query("INSERT INTO backlinks (target_id, source_path, target_path) VALUES (?, ?, ?)")
            .bind(target_id)
            .bind(&backlink.source_path)
            .bind(&backlink.target_path)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_error)?;
    }

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

async fn load_cached_graph(
    pool: &SqlitePool,
    target_id: i64,
) -> Result<(Vec<Node>, Vec<Edge>), BrainError> {
    let node_rows = sqlx::query(
        "SELECT node_id, title, summary, node_type, tags_json, x, y, path, sha
         FROM nodes WHERE target_id = ? ORDER BY node_id ASC",
    )
    .bind(target_id)
    .fetch_all(pool)
    .await
    .map_err(sqlx_error)?;

    let mut nodes = Vec::with_capacity(node_rows.len());
    for row in node_rows {
        let tags_json = row.get::<String, _>("tags_json");
        let tags = serde_json::from_str::<Vec<String>>(&tags_json)
            .map_err(|error| BrainError::parse(format!("projection tags parse: {error}")))?;
        nodes.push(Node {
            id: row.get::<i64, _>("node_id") as u32,
            title: row.get::<String, _>("title"),
            summary: row.get::<String, _>("summary"),
            node_type: row.get::<String, _>("node_type"),
            tags,
            x: row.get::<f64, _>("x") as f32,
            y: row.get::<f64, _>("y") as f32,
            path: row.get::<String, _>("path"),
            sha: row.get::<String, _>("sha"),
        });
    }

    let edge_rows = sqlx::query(
        "SELECT from_id, to_id FROM edges WHERE target_id = ? ORDER BY from_id ASC, to_id ASC",
    )
    .bind(target_id)
    .fetch_all(pool)
    .await
    .map_err(sqlx_error)?;
    let edges = edge_rows
        .into_iter()
        .map(|row| Edge {
            from: row.get::<i64, _>("from_id") as u32,
            to: row.get::<i64, _>("to_id") as u32,
        })
        .collect();

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
}
