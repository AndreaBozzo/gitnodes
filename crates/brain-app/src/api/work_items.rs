use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use brain_domain::{ExternalWorkItemBinding, TargetRef, WorkItem, WorkItemKind, WorkItemState};

use super::WriteResult;
#[cfg(feature = "ssr")]
use super::sfe;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WorkItemQueryFilters {
    #[serde(default)]
    pub brain_ids: Vec<String>,
    #[serde(default)]
    pub kinds: Vec<WorkItemKind>,
    #[serde(default)]
    pub states: Vec<WorkItemState>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub assignees: Vec<String>,
    #[serde(default)]
    pub content_paths: Vec<String>,
}

/// Parametric read side for operational items materialized in SQLite.
#[server(
    ListWorkItems,
    "/api",
    input = server_fn::codec::Json,
    endpoint = "list_work_items"
)]
pub async fn list_work_items(
    target: TargetRef,
    filters: WorkItemQueryFilters,
) -> Result<Vec<WorkItem>, ServerFnError> {
    use crate::server::session;

    let _ = session::require_authenticated().await.map_err(sfe)?;
    let target = super::target_from_ref(target).map_err(sfe)?;
    crate::server::projection::list_work_items(
        &target,
        &crate::server::projection::WorkItemFilters {
            brain_ids: filters.brain_ids,
            kinds: filters.kinds,
            states: filters.states,
            labels: filters.labels,
            assignees: filters.assignees,
            content_paths: filters.content_paths,
        },
    )
    .await
    .map_err(sfe)
}

#[server(LoadWorkItemByPath, "/api", endpoint = "load_work_item_by_path")]
pub async fn load_work_item_by_path(
    target: TargetRef,
    path: String,
) -> Result<Option<WorkItem>, ServerFnError> {
    use crate::server::session;

    let _ = session::require_authenticated().await.map_err(sfe)?;
    let target = super::target_from_ref(target).map_err(sfe)?;
    crate::server::projection::load_work_item_by_path(&target, &path)
        .await
        .map_err(sfe)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkItemComment {
    pub author: String,
    pub author_url: String,
    pub created_at: String,
    pub updated_at: String,
    pub body_markdown: String,
    pub body_html: String,
    pub url: String,
}

/// Load comments for the GitHub issue bound to a work item. Non-bound or
/// non-GitHub work items return an empty list so the UI can render one
/// collapsible "Comments" surface without provider-specific branching.
#[server(LoadWorkItemComments, "/api", endpoint = "load_work_item_comments")]
pub async fn load_work_item_comments(
    target: TargetRef,
    brain_id: String,
) -> Result<Vec<WorkItemComment>, ServerFnError> {
    use crate::server::session;

    session::__assert_gated();
    load_work_item_comments_inner(target, brain_id)
        .await
        .map_err(sfe)
}

#[cfg(feature = "ssr")]
async fn load_work_item_comments_inner(
    target: TargetRef,
    brain_id: String,
) -> Result<Vec<WorkItemComment>, brain_domain::BrainError> {
    use crate::server::session;
    use brain_domain::ExternalWorkItemSystem;

    let (_s, token) = session::require_session_and_token().await?;
    let target = super::target_from_ref(target)?;
    let storage = session::storage_for(target.clone())?;
    let Some(item) =
        crate::server::projection::load_work_item_by_brain_id(&target, &brain_id).await?
    else {
        return Ok(Vec::new());
    };
    let Some(binding) = item.external_binding else {
        return Ok(Vec::new());
    };
    if binding.system != ExternalWorkItemSystem::Github {
        return Ok(Vec::new());
    }

    storage
        .issue_comments(&token, &binding.project, &binding.item_key)
        .await
        .map(|comments| {
            comments
                .into_iter()
                .map(|comment| WorkItemComment {
                    author: comment.user.login,
                    author_url: comment.user.html_url,
                    created_at: comment.created_at,
                    updated_at: comment.updated_at,
                    body_html: crate::markdown::render(&comment.body),
                    body_markdown: comment.body,
                    url: comment.html_url,
                })
                .collect()
        })
}

/// Transition a work item to a new state. Updates frontmatter on the forge in
/// a single commit, then patches the local projection. For 3.2-alpha this only
/// touches the markdown file + projection; provider-side mutation was added in
/// the bidirectional sync pass below.
#[server(TransitionWorkItem, "/api", endpoint = "transition_work_item")]
pub async fn transition_work_item(
    target: TargetRef,
    brain_id: String,
    new_state: WorkItemState,
) -> Result<WorkItemMutationResult, ServerFnError> {
    use crate::server::session;

    session::__assert_gated();
    apply_work_item_mutation(target, brain_id, WorkItemMutation::State(new_state))
        .await
        .map_err(sfe)
}

/// Replace the assignees list on a work item. Same semantics as
/// `transition_work_item` (frontmatter + projection only in this slice).
#[server(
    AssignWorkItem,
    "/api",
    input = server_fn::codec::Json,
    endpoint = "assign_work_item"
)]
pub async fn assign_work_item(
    target: TargetRef,
    brain_id: String,
    assignees: Vec<String>,
) -> Result<WorkItemMutationResult, ServerFnError> {
    use crate::server::session;

    session::__assert_gated();
    apply_work_item_mutation(target, brain_id, WorkItemMutation::Assignees(assignees))
        .await
        .map_err(sfe)
}

/// Set or clear the external binding of a work item. Pass `None` to unbind.
#[server(
    BindWorkItem,
    "/api",
    input = server_fn::codec::Json,
    endpoint = "bind_work_item"
)]
pub async fn bind_work_item(
    target: TargetRef,
    brain_id: String,
    binding: Option<ExternalWorkItemBinding>,
) -> Result<WorkItemMutationResult, ServerFnError> {
    use crate::server::session;

    session::__assert_gated();
    apply_work_item_mutation(target, brain_id, WorkItemMutation::Binding(binding))
        .await
        .map_err(sfe)
}

#[cfg(feature = "ssr")]
#[derive(Clone)]
enum WorkItemMutation {
    State(WorkItemState),
    Assignees(Vec<String>),
    Binding(Option<ExternalWorkItemBinding>),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkItemMutationResult {
    pub item: WorkItem,
    pub write: WriteResult,
}

#[cfg(feature = "ssr")]
impl WorkItemMutation {
    fn audit_kind(&self) -> &'static str {
        match self {
            WorkItemMutation::State(_) => "work_item_transition",
            WorkItemMutation::Assignees(_) => "work_item_assign",
            WorkItemMutation::Binding(_) => "work_item_bind",
        }
    }

    fn is_binding(&self) -> bool {
        matches!(self, WorkItemMutation::Binding(_))
    }
}

#[cfg(feature = "ssr")]
async fn apply_work_item_mutation(
    target: TargetRef,
    brain_id: String,
    mutation: WorkItemMutation,
) -> Result<WorkItemMutationResult, brain_domain::BrainError> {
    use crate::server::session;

    use super::write_orchestrator::{open_write_pr, prepare_pr_write, should_fallback_to_pr};

    super::limits::check_len("Work item id", &brain_id, super::limits::MAX_FIELD_LEN)?;
    if let WorkItemMutation::Assignees(ref names) = mutation {
        for name in names {
            super::limits::check_len("Assignee", name, super::limits::MAX_FIELD_LEN)?;
        }
    }

    let (s, token) = session::require_session_and_token().await?;
    let user = session::session_user_or_fallback(&s).await;
    let target = super::target_from_ref(target)?;
    let storage = session::storage_for(target.clone())?;

    let permissions = storage.repository_permissions(&token).await?;
    if permissions.push {
        match apply_work_item_mutation_inner(
            &token,
            &user,
            &target,
            &storage,
            brain_id.clone(),
            mutation.clone(),
            true,
            true,
            true,
        )
        .await
        {
            Ok(item) => {
                let path = item.content_path.clone().unwrap_or_default();
                return Ok(WorkItemMutationResult {
                    item,
                    write: WriteResult::direct(path),
                });
            }
            Err(error) if should_fallback_to_pr(&error) => {}
            Err(error) => return Err(error),
        }
    }

    let current = crate::server::projection::load_work_item_by_brain_id(&target, &brain_id)
        .await?
        .ok_or_else(|| {
            brain_domain::BrainError::parse(format!("work item not found: {brain_id}"))
        })?;
    let path = current.content_path.clone().ok_or_else(|| {
        brain_domain::BrainError::parse(format!("work item {brain_id} has no content path"))
    })?;
    let plan = prepare_pr_write(
        &storage,
        &token,
        &user,
        &target,
        "work-item",
        &path,
        permissions.push,
    )
    .await?;
    let item = apply_work_item_mutation_inner(
        &token,
        &user,
        &target,
        &plan.storage,
        brain_id.clone(),
        mutation,
        false,
        false,
        false,
    )
    .await?;
    let pr = open_write_pr(
        &storage,
        &token,
        &plan,
        &format!("Propose work item update {brain_id} via Brain UI"),
        &format!("Brain UI could not update `{path}` directly on `{}` and proposed the work item change through a pull request instead.", target.branch),
    )
    .await?;
    crate::server::audit::log(
        "propose_work_item_mutation",
        Some(&user),
        &format!("{brain_id} via PR #{}", pr.number),
    )
    .await;
    Ok(WorkItemMutationResult {
        item,
        write: WriteResult::pull_request(path, plan.branch, pr.number, pr.html_url),
    })
}

#[cfg(feature = "ssr")]
pub(crate) async fn apply_provider_work_item_update(
    token: &str,
    user: &str,
    target: &brain_domain::TargetConfig,
    storage: &brain_storage::GithubStorage,
    brain_id: &str,
    state: Option<WorkItemState>,
    assignees: Option<Vec<String>>,
) -> Result<Option<WorkItem>, brain_domain::BrainError> {
    use brain_domain::WorkItemSystemOfRecord;

    let current = crate::server::projection::load_work_item_by_brain_id(target, brain_id).await?;
    let Some(current) = current else {
        return Ok(None);
    };
    if current.system_of_record == WorkItemSystemOfRecord::Brain {
        return Ok(None);
    }

    let mut last = None;
    if let Some(state) = state
        && current.state != state
    {
        last = Some(
            apply_work_item_mutation_inner(
                token,
                user,
                target,
                storage,
                brain_id.to_string(),
                WorkItemMutation::State(state),
                false,
                true,
                false,
            )
            .await?,
        );
    }
    if let Some(assignees) = assignees
        && current.assignees != assignees
    {
        last = Some(
            apply_work_item_mutation_inner(
                token,
                user,
                target,
                storage,
                brain_id.to_string(),
                WorkItemMutation::Assignees(assignees),
                false,
                true,
                false,
            )
            .await?,
        );
    }

    Ok(last)
}

#[cfg(feature = "ssr")]
#[allow(clippy::too_many_arguments)]
async fn apply_work_item_mutation_inner(
    token: &str,
    user: &str,
    target: &brain_domain::TargetConfig,
    storage: &brain_storage::GithubStorage,
    brain_id: String,
    mutation: WorkItemMutation,
    sync_provider: bool,
    patch_projection: bool,
    publish_event: bool,
) -> Result<WorkItem, brain_domain::BrainError> {
    use brain_domain::BrainError;
    use brain_storage::Storage;
    use serde_yaml::Value;
    use std::collections::BTreeMap;

    let work_item = crate::server::projection::load_work_item_by_brain_id(target, &brain_id)
        .await?
        .ok_or_else(|| BrainError::parse(format!("work item not found: {brain_id}")))?;
    let path = work_item
        .content_path
        .clone()
        .ok_or_else(|| BrainError::parse(format!("work item {brain_id} has no content path")))?;

    let (content, sha) = storage.read_file(token, &path).await?;
    let (front_raw, body) = brain_domain::split_frontmatter(&content);
    if front_raw.trim().is_empty() {
        return Err(BrainError::parse(format!(
            "work item {brain_id} has no frontmatter to patch"
        )));
    }

    let mut map: BTreeMap<String, Value> = serde_yaml::from_str(front_raw)
        .map_err(|error| BrainError::parse(format!("frontmatter parse: {error}")))?;

    match &mutation {
        WorkItemMutation::State(state) => {
            let serialized = serde_yaml::to_value(state)
                .map_err(|error| BrainError::parse(format!("state serialize: {error}")))?;
            map.insert("status".to_string(), serialized);
            map.remove("state");
        }
        WorkItemMutation::Assignees(assignees) => {
            let serialized = serde_yaml::to_value(assignees)
                .map_err(|error| BrainError::parse(format!("assignees serialize: {error}")))?;
            map.insert("assignees".to_string(), serialized);
        }
        WorkItemMutation::Binding(binding) => match binding {
            Some(binding) => {
                let serialized = serde_yaml::to_value(binding)
                    .map_err(|error| BrainError::parse(format!("binding serialize: {error}")))?;
                map.insert("external_binding".to_string(), serialized);
            }
            None => {
                map.remove("external_binding");
            }
        },
    }

    let new_front = serde_yaml::to_string(&map)
        .map_err(|error| BrainError::parse(format!("frontmatter serialize: {error}")))?;
    let new_content = format!("---\n{}---\n{}", new_front, body);
    let commit_msg = match &mutation {
        WorkItemMutation::State(state) => format!(
            "chore({brain_id}): set state to {state:?} via Brain UI",
            state = state
        ),
        WorkItemMutation::Assignees(_) => {
            format!("chore({brain_id}): update assignees via Brain UI")
        }
        WorkItemMutation::Binding(Some(_)) => {
            format!("chore({brain_id}): bind external item via Brain UI")
        }
        WorkItemMutation::Binding(None) => {
            format!("chore({brain_id}): unbind external item via Brain UI")
        }
    };
    let author_email = format!("{user}@users.noreply.github.com");

    storage
        .save_file(
            token,
            &path,
            &new_content,
            Some(&sha),
            &commit_msg,
            user,
            &author_email,
        )
        .await?;

    crate::server::audit::log(mutation.audit_kind(), Some(user), &path).await;

    if patch_projection {
        match &mutation {
            WorkItemMutation::State(state) => {
                crate::server::projection::update_work_item_state(target, &brain_id, state).await?;
            }
            WorkItemMutation::Assignees(assignees) => {
                crate::server::projection::update_work_item_assignees(target, &brain_id, assignees)
                    .await?;
            }
            WorkItemMutation::Binding(binding) => {
                crate::server::projection::upsert_work_item_binding(
                    target,
                    &brain_id,
                    binding.as_ref(),
                )
                .await?;
            }
        }
    }

    if sync_provider {
        let config = crate::knowledge::config_loader::load(target, token).await;
        if let Err(error) =
            sync_work_item_provider(storage, token, &config, &work_item, &mutation, user).await
        {
            crate::server::audit::log(
                "work_item_provider_sync_error",
                Some(user),
                &format!("{brain_id}: {error}"),
            )
            .await;
        }
    }

    if publish_event && let Some(bus) = crate::server::sse::global() {
        let event = if mutation.is_binding() {
            crate::server::sse::BrainEvent::BindingUpdated {
                target: brain_domain::TargetRef::from(target),
                brain_id: brain_id.clone(),
                content_path: Some(path.clone()),
            }
        } else {
            crate::server::sse::BrainEvent::WorkItemUpdated {
                target: brain_domain::TargetRef::from(target),
                brain_id: brain_id.clone(),
                content_path: Some(path.clone()),
            }
        };
        bus.send(event);
    }

    if patch_projection {
        crate::server::projection::load_work_item_by_brain_id(target, &brain_id)
            .await?
            .ok_or_else(|| BrainError::other("work item disappeared after mutation"))
    } else {
        Ok(work_item_with_mutation(work_item, &mutation))
    }
}

#[cfg(feature = "ssr")]
fn work_item_with_mutation(mut item: WorkItem, mutation: &WorkItemMutation) -> WorkItem {
    match mutation {
        WorkItemMutation::State(state) => item.state = state.clone(),
        WorkItemMutation::Assignees(assignees) => item.assignees = assignees.clone(),
        WorkItemMutation::Binding(binding) => item.external_binding = binding.clone(),
    }
    item
}

#[cfg(feature = "ssr")]
async fn sync_work_item_provider(
    storage: &brain_storage::GithubStorage,
    token: &str,
    config: &brain_domain::BrainConfig,
    item: &WorkItem,
    mutation: &WorkItemMutation,
    user: &str,
) -> Result<(), brain_domain::BrainError> {
    use brain_domain::{ExternalWorkItemSystem, WorkItemSystemOfRecord};

    if item.system_of_record == WorkItemSystemOfRecord::Brain {
        return Ok(());
    }
    let Some(binding) = item.external_binding.as_ref() else {
        return Ok(());
    };
    if binding.system != ExternalWorkItemSystem::Github {
        return Ok(());
    }

    let mut patch = brain_storage::GithubIssuePatch::default();
    match mutation {
        WorkItemMutation::State(state) => {
            patch.state = Some(github_issue_state_for(state).to_string());
            match github_labels_for_state(storage, token, config, item, state).await {
                Ok(labels) => patch.labels = labels,
                Err(error) => {
                    crate::server::audit::log(
                        "work_item_provider_label_sync_error",
                        Some(user),
                        &format!("{}: {error}", item.brain_id),
                    )
                    .await;
                }
            }
        }
        WorkItemMutation::Assignees(assignees) => {
            patch.assignees = Some(assignees.clone());
        }
        WorkItemMutation::Binding(_) => return Ok(()),
    }

    storage
        .patch_issue(token, &binding.project, &binding.item_key, &patch)
        .await
}

#[cfg(feature = "ssr")]
async fn github_labels_for_state(
    storage: &brain_storage::GithubStorage,
    token: &str,
    config: &brain_domain::BrainConfig,
    item: &WorkItem,
    state: &WorkItemState,
) -> Result<Option<Vec<String>>, brain_domain::BrainError> {
    use std::collections::HashSet;

    let Some(binding) = item.external_binding.as_ref() else {
        return Ok(None);
    };
    let Some(spec) = config.labels_for_kind(&item.kind) else {
        return Ok(None);
    };
    if spec.state_labels.is_empty() {
        return Ok(None);
    }

    let managed_state_labels: HashSet<&str> =
        spec.state_labels.values().map(String::as_str).collect();
    let mut labels = storage
        .issue_labels(token, &binding.project, &binding.item_key)
        .await?
        .into_iter()
        .filter(|label| !managed_state_labels.contains(label.as_str()))
        .collect::<Vec<_>>();

    if !labels.iter().any(|label| label == &spec.kind_label) {
        labels.push(spec.kind_label.clone());
    }
    if let Some(label) = spec.state_labels.get(state)
        && !labels.iter().any(|existing| existing == label)
    {
        labels.push(label.clone());
    }
    labels.sort();
    labels.dedup();
    Ok(Some(labels))
}

#[cfg(feature = "ssr")]
fn github_issue_state_for(state: &WorkItemState) -> &'static str {
    match state {
        WorkItemState::Done | WorkItemState::Cancelled => "closed",
        _ => "open",
    }
}
