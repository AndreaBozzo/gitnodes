use brain_domain::{BrainError, Edge};
use sqlx::{QueryBuilder, Sqlite};

use super::{
    SQLITE_MAX_VARIABLES, enum_str,
    links::Backlink,
    nodes::NodeInsertRow,
    rebuild::{NodeAuthorRow, ProjectionFile},
    sqlx_error,
    work_items::{ProjectedWorkItem, ProjectedWorkItemBinding},
};

pub(super) async fn bulk_insert_files(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    target_id: i64,
    files: &[ProjectionFile],
) -> Result<(), BrainError> {
    if files.is_empty() {
        return Ok(());
    }

    for chunk in files.chunks(max_rows_per_insert(6)) {
        let mut query = QueryBuilder::<Sqlite>::new(
            "INSERT INTO files (target_id, path, sha, size_bytes, body_text, frontmatter_json, updated_at) ",
        );
        query.push_values(chunk, |mut row, file| {
            row.push_bind(target_id)
                .push_bind(&file.path)
                .push_bind(&file.sha)
                .push_bind(file.size_bytes)
                .push_bind(&file.body_text)
                .push_bind(&file.frontmatter_json)
                .push("CURRENT_TIMESTAMP");
        });
        query.build().execute(&mut **tx).await.map_err(sqlx_error)?;
    }

    Ok(())
}

pub(super) async fn bulk_insert_nodes(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    target_id: i64,
    rows: &[NodeInsertRow],
) -> Result<(), BrainError> {
    if rows.is_empty() {
        return Ok(());
    }

    for chunk in rows.chunks(max_rows_per_insert(13)) {
        let mut query = QueryBuilder::<Sqlite>::new(
            "INSERT INTO nodes (
                target_id, node_id, title, summary, node_type, tags_json, x, y, path, sha, is_virtual, body_text, frontmatter_json
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
                .push_bind(node.is_virtual)
                .push_bind(&node.body_text)
                .push_bind(&node.frontmatter_json);
        });
        query.build().execute(&mut **tx).await.map_err(sqlx_error)?;
    }

    Ok(())
}

pub(super) async fn bulk_insert_node_authors(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    target_id: i64,
    rows: &[NodeAuthorRow],
) -> Result<(), BrainError> {
    if rows.is_empty() {
        return Ok(());
    }

    for chunk in rows.chunks(max_rows_per_insert(4)) {
        let mut query = QueryBuilder::<Sqlite>::new(
            "INSERT OR IGNORE INTO node_authors (target_id, node_id, author, role) ",
        );
        query.push_values(chunk, |mut bind, author| {
            bind.push_bind(target_id)
                .push_bind(author.node_id)
                .push_bind(&author.author)
                .push_bind(&author.role);
        });
        query.build().execute(&mut **tx).await.map_err(sqlx_error)?;
    }

    Ok(())
}

pub(super) async fn bulk_insert_edges(
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

pub(super) async fn bulk_insert_backlinks(
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

pub(super) async fn bulk_insert_work_items(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    target_id: i64,
    work_items: &[ProjectedWorkItem],
) -> Result<(), BrainError> {
    if work_items.is_empty() {
        return Ok(());
    }

    for chunk in work_items.chunks(max_rows_per_insert(9)) {
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

pub(super) async fn bulk_insert_work_item_bindings(
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
