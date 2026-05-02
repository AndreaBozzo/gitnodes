use std::collections::{HashMap, HashSet};
use std::path::Path;

use brain_domain::{BrainConfig, BrainError, Edge, Node, TargetKey};
use brain_graph::{RawFile, build_graph, parse_file};
use brain_storage::GithubStorage;
use sqlx::SqlitePool;

use super::{
    bulk_insert::{
        bulk_insert_backlinks, bulk_insert_edges, bulk_insert_files, bulk_insert_nodes,
        bulk_insert_work_item_bindings, bulk_insert_work_items,
    },
    links::{Backlink, resolve_link_path},
    nodes::load_cached_graph,
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
pub(super) struct ProjectionSnapshot {
    files: Vec<ProjectionFile>,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    backlinks: Vec<Backlink>,
    work_items: Vec<ProjectedWorkItem>,
    work_item_bindings: Vec<ProjectedWorkItemBinding>,
}

impl ProjectionSnapshot {
    pub(super) fn from_raw_files(raw_files: &[RawFile], config: &BrainConfig) -> Self {
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
pub(super) struct ProjectionFile {
    pub(super) path: String,
    pub(super) sha: String,
    pub(super) size_bytes: i64,
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
