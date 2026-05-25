use std::collections::HashSet;

use brain_domain::{BrainError, Edge, Node, TargetConfig};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool, sqlite::SqliteRow};

use super::{normalize_path_prefix, pool, sqlx_error, target::ensure_target_id};

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

pub async fn list_nodes(
    target: &TargetConfig,
    filters: &NodeFilters,
) -> Result<Vec<Node>, BrainError> {
    let pool = pool()?;
    let target_id = ensure_target_id(pool, target).await?;
    list_nodes_from_pool(pool, target_id, filters).await
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

pub(super) async fn list_nodes_from_pool(
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

pub(super) async fn list_edges_from_pool(
    pool: &SqlitePool,
    target_id: i64,
) -> Result<Vec<Edge>, BrainError> {
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

pub(super) async fn load_cached_graph(
    pool: &SqlitePool,
    target_id: i64,
) -> Result<(Vec<Node>, Vec<Edge>), BrainError> {
    let nodes = list_nodes_from_pool(pool, target_id, &NodeFilters::default()).await?;
    let edges = list_edges_from_pool(pool, target_id).await?;
    Ok((nodes, edges))
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

#[derive(Clone, Debug)]
pub(super) struct NodeInsertRow {
    pub(super) node_id: i64,
    pub(super) title: String,
    pub(super) summary: String,
    pub(super) node_type: String,
    pub(super) tags_json: String,
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) path: String,
    pub(super) sha: String,
    pub(super) blob_sha: Option<String>,
    pub(super) is_virtual: bool,
    pub(super) body_text: Option<String>,
    pub(super) frontmatter_json: Option<String>,
}
