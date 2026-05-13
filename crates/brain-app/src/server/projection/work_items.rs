use std::collections::{BTreeMap, HashSet};

use brain_domain::{
    BrainConfig, BrainError, ExternalWorkItemBinding, TargetConfig, WorkItem, WorkItemKind,
    WorkItemState, WorkItemSystemOfRecord,
};
use brain_graph::RawFile;
use serde_yaml::Value;
use sqlx::{QueryBuilder, Row, SqlitePool, sqlite::SqliteRow};

use super::{enum_str, parse_enum_str, pool, sqlx_error, target::ensure_target_id};

pub async fn load_work_item_by_path(
    target: &TargetConfig,
    path: &str,
) -> Result<Option<WorkItem>, BrainError> {
    read_work_item_by_path(target, path).await
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
/// having already updated the markdown frontmatter on the forge - this keeps
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

pub(super) async fn load_work_item_by_path_from_pool(
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

pub(super) async fn list_work_items_from_pool(
    pool: &SqlitePool,
    target_id: i64,
    filters: &WorkItemFilters,
) -> Result<Vec<WorkItem>, BrainError> {
    let mut query = QueryBuilder::<sqlx::Sqlite>::new(
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

pub(super) fn project_work_item(
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
    let state = enum_field::<WorkItemState>(&map, "status")
        .or_else(|| enum_field::<WorkItemState>(&map, "state"))
        .unwrap_or(WorkItemState::Todo);
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
pub(super) struct ProjectedWorkItem {
    pub(super) brain_id: String,
    pub(super) kind: WorkItemKind,
    pub(super) title: String,
    pub(super) state: WorkItemState,
    pub(super) labels_json: String,
    pub(super) assignees_json: String,
    pub(super) content_path: Option<String>,
    pub(super) system_of_record: WorkItemSystemOfRecord,
}

#[derive(Clone, Debug)]
pub(super) struct ProjectedWorkItemBinding {
    pub(super) brain_id: String,
    pub(super) binding: ExternalWorkItemBinding,
}
