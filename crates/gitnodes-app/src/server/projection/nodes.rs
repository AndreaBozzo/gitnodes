use std::collections::HashSet;

use gitnodes_domain::{BrainError, Edge, Node, TargetConfig};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool, sqlite::SqliteRow};

use super::{normalize_path_prefix, pool, sqlx_error, target::ensure_target_id};

#[derive(Clone, Debug)]
pub struct NodeFilters {
    pub node_types: Vec<String>,
    pub tags: Vec<String>,
    pub paths: Vec<String>,
    pub path_prefix: Option<String>,
    pub include_virtual: bool,
    /// Cap on rows returned. Pushed into SQL only when no tag filter is set,
    /// since tags are matched in Rust after the query; with tags the caller
    /// must still bound the post-filtered result itself.
    pub limit: Option<usize>,
}

impl Default for NodeFilters {
    fn default() -> Self {
        Self {
            node_types: Vec::new(),
            tags: Vec::new(),
            paths: Vec::new(),
            path_prefix: None,
            include_virtual: true,
            limit: None,
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

/// One edge incident to a node, resolved to the node on the other end. Lets an
/// agent traverse the graph from a known path without pulling the whole edge set.
#[derive(Clone, Debug)]
pub struct Neighbor {
    pub path: String,
    pub title: String,
    pub node_type: String,
    pub is_virtual: bool,
    /// `"outgoing"` (this node links out) or `"incoming"` (links here).
    pub direction: &'static str,
    /// Edge kind storage key: `body`, `frontmatter`, or `tag`.
    pub kind: String,
}

/// Resolve every edge touching `path` to the node on the other end. Returns
/// `None` when `path` is not a node in the projection (so callers can surface a
/// not-found distinct from a node that simply has no links).
pub async fn node_neighbors(
    target: &TargetConfig,
    path: &str,
) -> Result<Option<Vec<Neighbor>>, BrainError> {
    let pool = pool()?;
    let target_id = ensure_target_id(pool, target).await?;
    node_neighbors_from_pool(pool, target_id, path).await
}

pub(super) async fn node_neighbors_from_pool(
    pool: &SqlitePool,
    target_id: i64,
    path: &str,
) -> Result<Option<Vec<Neighbor>>, BrainError> {
    let node_id: Option<i64> =
        sqlx::query_scalar("SELECT node_id FROM nodes WHERE target_id = ? AND path = ?")
            .bind(target_id)
            .bind(path)
            .fetch_optional(pool)
            .await
            .map_err(sqlx_error)?;
    let Some(node_id) = node_id else {
        return Ok(None);
    };

    let mut neighbors = fetch_neighbors(pool, target_id, node_id, "outgoing").await?;
    neighbors.extend(fetch_neighbors(pool, target_id, node_id, "incoming").await?);
    Ok(Some(neighbors))
}

/// `direction` is `"outgoing"` (anchor is the edge source) or anything else
/// (anchor is the edge target). The column names it selects are derived from
/// this fixed match — never from caller input — so the formatted SQL is safe.
async fn fetch_neighbors(
    pool: &SqlitePool,
    target_id: i64,
    node_id: i64,
    direction: &'static str,
) -> Result<Vec<Neighbor>, BrainError> {
    let (anchor_col, endpoint_col) = if direction == "outgoing" {
        ("from_id", "to_id")
    } else {
        ("to_id", "from_id")
    };
    let sql = format!(
        "SELECT n.path AS path, n.title AS title, n.node_type AS node_type, \
                n.is_virtual AS is_virtual, e.kind AS kind \
         FROM edges e \
         JOIN nodes n ON n.node_id = e.{endpoint_col} AND n.target_id = e.target_id \
         WHERE e.target_id = ? AND e.{anchor_col} = ? \
         ORDER BY n.path ASC, e.kind ASC"
    );
    let rows = sqlx::query(&sql)
        .bind(target_id)
        .bind(node_id)
        .fetch_all(pool)
        .await
        .map_err(sqlx_error)?;
    Ok(rows
        .into_iter()
        .map(|row| Neighbor {
            path: row.get::<String, _>("path"),
            title: row.get::<String, _>("title"),
            node_type: row.get::<String, _>("node_type"),
            is_virtual: row.get::<i64, _>("is_virtual") != 0,
            direction,
            kind: row.get::<String, _>("kind"),
        })
        .collect())
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

    // Tags are matched in Rust below, so a SQL LIMIT would truncate before that
    // filter and under-return. Only push it down when no tag filter is active.
    if filters.tags.is_empty()
        && let Some(limit) = filters.limit
    {
        query.push(" LIMIT ");
        query.push_bind(limit as i64);
    }

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
        "SELECT from_id, to_id, kind FROM edges WHERE target_id = ? ORDER BY from_id ASC, to_id ASC, kind ASC",
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
            kind: gitnodes_domain::EdgeKind::from_storage_key(&row.get::<String, _>("kind")),
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
