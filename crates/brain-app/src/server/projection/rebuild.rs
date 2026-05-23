use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

use brain_domain::{BrainConfig, BrainError, Edge, Node, TargetKey, split_frontmatter};
use brain_graph::{RawFile, build_graph, parse_file};
use brain_storage::GithubStorage;
use sqlx::SqlitePool;

use super::{
    bulk_insert::{
        bulk_insert_backlinks, bulk_insert_edges, bulk_insert_files, bulk_insert_node_authors,
        bulk_insert_nodes, bulk_insert_work_item_bindings, bulk_insert_work_items,
    },
    links::{Backlink, resolve_link_path},
    nodes::{NodeInsertRow, load_cached_graph},
    pool, sqlx_error,
    sync_state::{load_sync_state, record_attempt, record_failure},
    target::ensure_target_id,
    work_items::{ProjectedWorkItem, ProjectedWorkItemBinding, project_work_item},
};

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

    let started = Instant::now();
    let result = async {
        let raw_files = storage.fetch_raw_files(token).await?;
        let snapshot = ProjectionSnapshot::from_raw_files(&raw_files, config);
        persist_snapshot(pool, target_id, &snapshot, reason).await?;
        Ok::<i64, BrainError>(started.elapsed().as_millis() as i64)
    }
    .await;

    match result {
        Ok(duration_ms) => {
            // Record total elapsed (fetch + parse + write) on the freshly
            // committed sync-state row. Best-effort: a failure here doesn't
            // invalidate the rebuild itself.
            if let Err(state_error) = sqlx::query(
                "UPDATE projection_sync_state SET last_rebuild_duration_ms = ? WHERE target_id = ?",
            )
            .bind(duration_ms)
            .bind(target_id)
            .execute(pool)
            .await
            {
                tracing::warn!(
                    target = %TargetKey::from(&target),
                    error = %state_error,
                    "failed to record projection rebuild duration"
                );
            }
            tracing::info!(
                target = %TargetKey::from(&target),
                reason,
                duration_ms,
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
pub(super) struct ProjectionSnapshot {
    pub(super) files: Vec<ProjectionFile>,
    pub(super) nodes: Vec<NodeInsertRow>,
    pub(super) edges: Vec<Edge>,
    pub(super) backlinks: Vec<Backlink>,
    pub(super) work_items: Vec<ProjectedWorkItem>,
    pub(super) work_item_bindings: Vec<ProjectedWorkItemBinding>,
    pub(super) node_authors: Vec<NodeAuthorRow>,
}

impl ProjectionSnapshot {
    pub(super) fn from_raw_files(raw_files: &[RawFile], config: &BrainConfig) -> Self {
        let (nodes, edges) = build_graph(raw_files, config);

        // Pre-extract body + frontmatter per file path so files/nodes can share
        // the same parse. `split_frontmatter` is the same helper brain-graph
        // uses internally, so the result is byte-identical to what FTS5 will
        // index next slice.
        let mut body_by_path: HashMap<String, String> = HashMap::with_capacity(raw_files.len());
        let mut frontmatter_json_by_path: HashMap<String, String> = HashMap::new();
        let mut frontmatter_value_by_path: HashMap<String, serde_yaml::Value> = HashMap::new();
        for file in raw_files {
            let (front, body) = split_frontmatter(&file.content);
            body_by_path.insert(file.path.clone(), body.to_string());
            if front.is_empty() {
                continue;
            }
            if let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(front) {
                if let Ok(json) = serde_json::to_string(&value) {
                    frontmatter_json_by_path.insert(file.path.clone(), json);
                }
                frontmatter_value_by_path.insert(file.path.clone(), value);
            }
        }

        let files = raw_files
            .iter()
            .map(|file| ProjectionFile {
                path: file.path.clone(),
                sha: file.sha.clone(),
                size_bytes: file.content.len() as i64,
                body_text: body_by_path.get(&file.path).cloned(),
                frontmatter_json: frontmatter_json_by_path.get(&file.path).cloned(),
            })
            .collect();

        // Build node rows. `serde_json::to_string` over `Vec<String>` can't
        // actually fail (no non-UTF-8 strings reach this path), but we skip +
        // log instead of unwrapping so a future change to `Node::tags` shape
        // can't panic the whole rebuild.
        let mut node_rows = Vec::with_capacity(nodes.len());
        for node in &nodes {
            let tags_json = match serde_json::to_string(&node.tags) {
                Ok(s) => s,
                Err(error) => {
                    tracing::warn!(
                        node_id = node.id,
                        path = %node.path,
                        %error,
                        "projection: skipping node with unserializable tags"
                    );
                    continue;
                }
            };
            let (body_text, frontmatter_json) = if node.path.is_empty() {
                (None, None)
            } else {
                (
                    body_by_path.get(&node.path).cloned(),
                    frontmatter_json_by_path.get(&node.path).cloned(),
                )
            };
            node_rows.push(NodeInsertRow {
                node_id: i64::from(node.id),
                title: node.title.clone(),
                summary: node.summary.clone(),
                node_type: node.node_type.clone(),
                tags_json,
                x: node.x as f64,
                y: node.y as f64,
                path: node.path.clone(),
                sha: node.sha.clone(),
                is_virtual: node.path.is_empty(),
                body_text,
                frontmatter_json,
            });
        }

        let mut node_authors = Vec::<NodeAuthorRow>::new();
        let mut seen_author = HashSet::<(i64, String, String)>::new();
        for node in &nodes {
            if node.path.is_empty() {
                continue;
            }
            let Some(front) = frontmatter_value_by_path.get(&node.path) else {
                continue;
            };
            let mapping = match front.as_mapping() {
                Some(m) => m,
                None => continue,
            };
            // Singular `author: name`.
            if let Some(name) = mapping
                .get(serde_yaml::Value::String("author".to_string()))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                let key = (i64::from(node.id), name.to_string(), "author".to_string());
                if seen_author.insert(key) {
                    node_authors.push(NodeAuthorRow {
                        node_id: i64::from(node.id),
                        author: name.to_string(),
                        role: "author".to_string(),
                    });
                }
            }
            // Plural `authors: [a, b]`.
            if let Some(seq) = mapping
                .get(serde_yaml::Value::String("authors".to_string()))
                .and_then(|v| v.as_sequence())
            {
                for entry in seq {
                    let Some(name) = entry.as_str().map(str::trim).filter(|s| !s.is_empty()) else {
                        continue;
                    };
                    let key = (i64::from(node.id), name.to_string(), "author".to_string());
                    if seen_author.insert(key) {
                        node_authors.push(NodeAuthorRow {
                            node_id: i64::from(node.id),
                            author: name.to_string(),
                            role: "author".to_string(),
                        });
                    }
                }
            }
        }

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
            nodes: node_rows,
            edges,
            backlinks,
            work_items,
            work_item_bindings,
            node_authors,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct ProjectionFile {
    pub(super) path: String,
    pub(super) sha: String,
    pub(super) size_bytes: i64,
    pub(super) body_text: Option<String>,
    pub(super) frontmatter_json: Option<String>,
}

#[derive(Clone, Debug)]
pub(super) struct NodeAuthorRow {
    pub(super) node_id: i64,
    pub(super) author: String,
    pub(super) role: String,
}

pub(super) async fn persist_snapshot(
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
    sqlx::query("DELETE FROM node_authors WHERE target_id = ?")
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
    bulk_insert_node_authors(&mut tx, target_id, &snapshot.node_authors).await?;
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
