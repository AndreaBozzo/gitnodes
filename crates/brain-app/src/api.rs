use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::knowledge::types::{BrainFilePayload, Edge, Node};
use brain_domain::{
    BrainConfig, BrandConfig, ExternalWorkItemBinding, TargetConfig, ViewSpec, WorkItem,
    WorkItemKind, WorkItemState,
};
#[cfg(feature = "ssr")]
use brain_domain::{ExternalWorkItemSystem, WorkItemSystemOfRecord};

#[cfg(feature = "ssr")]
use brain_domain::BrainError;

#[cfg(feature = "ssr")]
fn sfe(e: BrainError) -> ServerFnError {
    ServerFnError::new(e.to_string())
}

/// Accept a user-supplied commit message only if it's non-empty after trim and
/// free of control characters (tabs, CR, LF, etc.). Cap at 200 chars to keep
/// subject lines sane. Returns `None` to signal "fall back to auto-message".
#[cfg(feature = "ssr")]
fn sanitize_commit_message(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.chars().any(|c| c.is_control() && c != ' ') {
        return None;
    }
    let mut out = trimmed.to_string();
    if out.chars().count() > 200 {
        out = out.chars().take(200).collect();
    }
    Some(out)
}

/// Canonical list of every `#[server]` fn in this crate. Single source of
/// truth for both `register_server_functions` (runtime registration) and the
/// `server_fns_registered_match_attributes` test (build-time guardrail).
///
/// **Adding a new `#[server]` fn requires adding its struct name here.** The
/// regression test fails the build otherwise — preventing the silent
/// release-mode 404 documented in caveat #9.
#[cfg(feature = "ssr")]
#[cfg_attr(not(test), allow(dead_code))]
const SERVER_FNS: &[&str] = &[
    "GetAppConfig",
    "LoadBrainConfig",
    "LoadAuditLog",
    "ListSessions",
    "RevokeSession",
    "GetCurrentUser",
    "LoadBrainTemplate",
    "ListNodes",
    "ListWorkItems",
    "ReadNode",
    "LoadBrainGraph",
    "LoadWorkItemByPath",
    "LoadWorkItemComments",
    "ReadBrainFile",
    "SaveBrainFile",
    "DeleteBrainFile",
    "RenameBrainFile",
    "UploadAsset",
    "ListBrainFolders",
    "RefreshBrainGraph",
    "GetWriteCapabilities",
    "ListAccessibleTargets",
    "LoadBrainGraphForTarget",
    "LoadBrainConfigForTarget",
    "TransitionWorkItem",
    "AssignWorkItem",
    "BindWorkItem",
    "ListViews",
    "SaveViews",
];

#[cfg(feature = "ssr")]
pub fn register_server_functions() {
    // LTO (`lto = true` in [profile.release]) strips the `inventory::submit!`
    // entries that `#[server]` relies on for automatic registration. Calling
    // `register_explicit` bypasses inventory and directly inserts each server
    // function into the global handler map.
    use leptos::server_fn::axum::register_explicit;
    register_explicit::<GetAppConfig>();
    register_explicit::<LoadBrainConfig>();
    register_explicit::<LoadAuditLog>();
    register_explicit::<ListSessions>();
    register_explicit::<RevokeSession>();
    register_explicit::<GetCurrentUser>();
    register_explicit::<LoadBrainTemplate>();
    register_explicit::<ListNodes>();
    register_explicit::<ListWorkItems>();
    register_explicit::<ReadNode>();
    register_explicit::<LoadBrainGraph>();
    register_explicit::<LoadWorkItemByPath>();
    register_explicit::<LoadWorkItemComments>();
    register_explicit::<ReadBrainFile>();
    register_explicit::<SaveBrainFile>();
    register_explicit::<DeleteBrainFile>();
    register_explicit::<RenameBrainFile>();
    register_explicit::<UploadAsset>();
    register_explicit::<ListBrainFolders>();
    register_explicit::<RefreshBrainGraph>();
    register_explicit::<GetWriteCapabilities>();
    register_explicit::<ListAccessibleTargets>();
    register_explicit::<LoadBrainGraphForTarget>();
    register_explicit::<LoadBrainConfigForTarget>();
    register_explicit::<TransitionWorkItem>();
    register_explicit::<AssignWorkItem>();
    register_explicit::<BindWorkItem>();
    register_explicit::<ListViews>();
    register_explicit::<SaveViews>();
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppConfig {
    pub target: TargetConfig,
    pub brand: BrandConfig,
}

#[server(GetAppConfig, "/api", endpoint = "get_app_config")]
pub async fn get_app_config() -> Result<AppConfig, ServerFnError> {
    use crate::server::session;
    let target = session::target_cfg().map_err(sfe)?;
    let brand = use_context::<BrandConfig>()
        .ok_or_else(|| sfe(BrainError::other("No brand config available")))?;
    Ok(AppConfig { target, brand })
}

#[server(LoadBrainConfig, "/api", endpoint = "load_brain_config")]
pub async fn load_brain_config() -> Result<BrainConfig, ServerFnError> {
    use crate::knowledge::config_loader;
    use crate::server::session;
    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let target = session::target_cfg().map_err(sfe)?;
    let cfg = config_loader::load(&target, &token).await;
    Ok((*cfg).clone())
}

/// Read-only list of saved views for the active target. Backed by the same
/// cached `BrainConfig` as the rest of the runtime, so it reflects the latest
/// committed state of `.brain-config.yml` without an extra fetch.
#[server(ListViews, "/api", endpoint = "list_views")]
pub async fn list_views() -> Result<Vec<ViewSpec>, ServerFnError> {
    use crate::knowledge::config_loader;
    use crate::server::session;
    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let target = session::target_cfg().map_err(sfe)?;
    let cfg = config_loader::load(&target, &token).await;
    Ok(cfg.views.clone())
}

/// Replace the entire `views` block in `.brain-config.yml` with the supplied
/// list. Other config fields (node_types, label_taxonomy, default_type) are
/// preserved by parsing → mutating → re-serializing the existing file. Routes
/// through the same permission-aware orchestrator as document saves: direct
/// commit when possible, PR fallback otherwise.
///
/// Returns the same `WriteResult` shape as `SaveBrainFile` so the admin UI can
/// render `Saved` / `Proposed via PR #...` consistently with the editor.
#[server(SaveViews, "/api", endpoint = "save_views")]
pub async fn save_views(views: Vec<ViewSpec>) -> Result<WriteResult, ServerFnError> {
    use crate::knowledge::config_loader;
    use crate::server::session;
    use brain_storage::Storage;

    let (s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let user = session::session_user_or_fallback(&s).await;
    let target = session::target_cfg().map_err(sfe)?;
    let storage = session::storage().map_err(sfe)?;

    let permissions = storage.repository_permissions(&token).await.map_err(sfe)?;
    if !(permissions.admin || permissions.maintain) {
        return Err(sfe(BrainError::other(
            "Editing views requires admin or maintain access on the repository.",
        )));
    }

    const CONFIG_PATH: &str = ".brain-config.yml";
    let (existing_raw, existing_sha) = match storage.read_file(&token, CONFIG_PATH).await {
        Ok((raw, sha)) => (raw, Some(sha)),
        Err(BrainError::NotFound(_)) => (String::new(), None),
        Err(e) => return Err(sfe(e)),
    };

    let mut cfg = if existing_raw.trim().is_empty() {
        BrainConfig::default()
    } else {
        BrainConfig::parse(&existing_raw).map_err(|e| {
            sfe(BrainError::other(format!(
                "current .brain-config.yml does not parse: {e}"
            )))
        })?
    };
    cfg.views = views;
    cfg.validate()
        .map_err(|e| sfe(BrainError::other(e.to_string())))?;

    let new_yaml = serde_yaml::to_string(&cfg)
        .map_err(|e| sfe(BrainError::other(format!("yaml serialize: {e}"))))?;
    let author_email = format!("{}@users.noreply.github.com", user);
    let commit_msg = "Update saved views via Brain UI".to_string();

    match save_file_permission_aware(
        &storage,
        &token,
        CONFIG_PATH,
        &new_yaml,
        existing_sha.as_deref(),
        &commit_msg,
        &user,
        &author_email,
        &target,
    )
    .await
    {
        Ok(result) => {
            match result.mode {
                WriteMode::Direct => {
                    crate::server::audit::log("update_views", Some(&user), CONFIG_PATH).await;
                    config_loader::invalidate(&(&target).into());
                    rebuild_projection_after_write(
                        &storage,
                        &target,
                        &token,
                        &user,
                        "update_views",
                    )
                    .await;
                }
                WriteMode::PullRequest => {
                    crate::server::audit::log(
                        "propose_views_update",
                        Some(&user),
                        &format!(
                            "{} via PR #{}",
                            CONFIG_PATH,
                            result
                                .pr_number
                                .map(|n| n.to_string())
                                .unwrap_or_else(|| "?".to_string())
                        ),
                    )
                    .await;
                }
            }
            Ok(result)
        }
        Err(e) => {
            crate::server::audit::log("api_error", Some(&user), &format!("save_views: {e}")).await;
            Err(sfe(e))
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: i64,
    pub ts: String,
    pub kind: String,
    pub actor: Option<String>,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionEntry {
    pub id: String,
    pub expiry_date: String,
}

#[server(LoadAuditLog, "/api", endpoint = "load_audit_log")]
pub async fn load_audit_log(
    kind: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<AuditEntry>, ServerFnError> {
    use crate::server::session;
    let _ = session::require_authenticated().await.map_err(sfe)?;
    let rows = crate::server::audit::recent(limit.unwrap_or(200), kind.as_deref())
        .await
        .map_err(|e| sfe(BrainError::other(format!("DB: {e}"))))?;
    Ok(rows
        .into_iter()
        .map(|r| AuditEntry {
            id: r.id,
            ts: r.ts,
            kind: r.kind,
            actor: r.actor,
            detail: r.detail,
        })
        .collect())
}

#[server(ListSessions, "/api", endpoint = "list_sessions")]
pub async fn list_sessions() -> Result<Vec<SessionEntry>, ServerFnError> {
    use crate::server::session;
    let _ = session::require_authenticated().await.map_err(sfe)?;
    let rows = crate::server::audit::list_sessions(100)
        .await
        .map_err(|e| sfe(BrainError::other(format!("DB: {e}"))))?;
    Ok(rows
        .into_iter()
        .map(|r| SessionEntry {
            id: r.id,
            expiry_date: r.expiry_date,
        })
        .collect())
}

#[server(RevokeSession, "/api", endpoint = "revoke_session")]
pub async fn revoke_session(id: String) -> Result<u64, ServerFnError> {
    use crate::server::session;
    let s = session::require_authenticated().await.map_err(sfe)?;
    let actor = crate::server::auth::get_session_user(&s).await;
    let n = crate::server::audit::revoke_session(&id)
        .await
        .map_err(|e| sfe(BrainError::other(format!("DB: {e}"))))?;
    crate::server::audit::log("revoke_session", actor.as_deref(), &id).await;
    Ok(n)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrainFile {
    pub path: String,
    pub sha: String,
    pub content: String,
    #[serde(default)]
    pub rendered_html: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WriteMode {
    Direct,
    PullRequest,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WriteResult {
    pub path: String,
    pub mode: WriteMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WriteCapabilities {
    pub can_read: bool,
    pub can_write_default_branch: bool,
    pub can_review_via_pr: bool,
    pub can_admin_config: bool,
}

#[server(GetWriteCapabilities, "/api", endpoint = "get_write_capabilities")]
pub async fn get_write_capabilities() -> Result<WriteCapabilities, ServerFnError> {
    use crate::server::session;

    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let storage = session::storage().map_err(sfe)?;
    let permissions = storage.repository_permissions(&token).await.map_err(sfe)?;
    Ok(WriteCapabilities {
        can_read: permissions.pull,
        can_write_default_branch: permissions.push,
        can_review_via_pr: permissions.pull,
        can_admin_config: permissions.admin || permissions.maintain,
    })
}

#[cfg(feature = "ssr")]
impl WriteResult {
    fn direct(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            mode: WriteMode::Direct,
            branch: None,
            pr_url: None,
            pr_number: None,
        }
    }

    fn pull_request(
        path: impl Into<String>,
        branch: impl Into<String>,
        pr_number: u64,
        pr_url: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            mode: WriteMode::PullRequest,
            branch: Some(branch.into()),
            pr_url: Some(pr_url.into()),
            pr_number: Some(pr_number),
        }
    }
}

#[server(GetCurrentUser, "/api", endpoint = "get_current_user")]
pub async fn get_current_user() -> Result<Option<String>, ServerFnError> {
    use crate::server::session;
    let s = session::session().map_err(sfe)?;
    Ok(crate::server::auth::get_session_user(&s).await)
}

#[server(LoadBrainTemplate, "/api", endpoint = "load_brain_template")]
pub async fn load_brain_template(node_type: String) -> Result<String, ServerFnError> {
    use crate::server::session;
    use brain_storage::Storage;
    let target = session::target_cfg().map_err(sfe)?;
    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let config = crate::knowledge::config_loader::load(&target, &token).await;
    let Some(filename) = config
        .lookup(&node_type)
        .and_then(|s| s.template_filename.as_deref())
    else {
        return Ok(String::new());
    };
    let storage = session::storage().map_err(sfe)?;
    let raw = storage.load_template(&token, filename).await.map_err(sfe)?;
    let (body, _front) = crate::markdown::split_frontmatter(&raw);
    Ok(body.trim_start_matches('\n').to_string())
}

fn default_include_virtual() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeQueryFilters {
    #[serde(default)]
    pub node_types: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default = "default_include_virtual")]
    pub include_virtual: bool,
}

impl Default for NodeQueryFilters {
    fn default() -> Self {
        Self {
            node_types: Vec::new(),
            tags: Vec::new(),
            paths: Vec::new(),
            include_virtual: true,
        }
    }
}

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

/// Parametric read side for graph nodes. This intentionally exposes the
/// target-scoped SQLite projection instead of causing a forge read; explicit
/// reconciliation stays behind `RefreshBrainGraph`, webhook handling, and
/// post-write rebuilds.
#[server(ListNodes, "/api", input = server_fn::codec::Json, endpoint = "list_nodes")]
pub async fn list_nodes(filters: NodeQueryFilters) -> Result<Vec<Node>, ServerFnError> {
    use crate::server::session;

    let _ = session::require_authenticated().await.map_err(sfe)?;
    let target = session::target_cfg().map_err(sfe)?;
    crate::server::projection::list_nodes(
        &target,
        &crate::server::projection::NodeFilters {
            node_types: filters.node_types,
            tags: filters.tags,
            paths: filters.paths,
            include_virtual: filters.include_virtual,
        },
    )
    .await
    .map_err(sfe)
}

/// Parametric read side for operational items materialized in SQLite.
#[server(
    ListWorkItems,
    "/api",
    input = server_fn::codec::Json,
    endpoint = "list_work_items"
)]
pub async fn list_work_items(
    filters: WorkItemQueryFilters,
) -> Result<Vec<WorkItem>, ServerFnError> {
    use crate::server::session;

    let _ = session::require_authenticated().await.map_err(sfe)?;
    let target = session::target_cfg().map_err(sfe)?;
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

/// Read one projected node by repo-relative path, without fetching file
/// content from GitHub. `ReadBrainFile` remains the markdown-content path.
#[server(ReadNode, "/api", endpoint = "read_node")]
pub async fn read_node(path: String) -> Result<Option<Node>, ServerFnError> {
    use crate::server::session;

    let _ = session::require_authenticated().await.map_err(sfe)?;
    let target = session::target_cfg().map_err(sfe)?;
    crate::server::projection::read_node(&target, &path)
        .await
        .map_err(sfe)
}

#[server(LoadBrainGraph, "/api", endpoint = "load_brain_graph")]
pub async fn load_brain_graph() -> Result<(Vec<Node>, Vec<Edge>), ServerFnError> {
    use crate::server::session;
    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let target = session::target_cfg().map_err(sfe)?;
    let config = crate::knowledge::config_loader::load(&target, &token).await;
    let storage = session::storage().map_err(sfe)?;
    crate::server::projection::load_graph(&storage, &token, &config)
        .await
        .map_err(sfe)
}

#[server(LoadWorkItemByPath, "/api", endpoint = "load_work_item_by_path")]
pub async fn load_work_item_by_path(path: String) -> Result<Option<WorkItem>, ServerFnError> {
    use crate::server::session;

    let _ = session::require_authenticated().await.map_err(sfe)?;
    let target = session::target_cfg().map_err(sfe)?;
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
    brain_id: String,
) -> Result<Vec<WorkItemComment>, ServerFnError> {
    load_work_item_comments_inner(brain_id).await.map_err(sfe)
}

#[cfg(feature = "ssr")]
async fn load_work_item_comments_inner(
    brain_id: String,
) -> Result<Vec<WorkItemComment>, BrainError> {
    use crate::server::session;

    let (_s, token) = session::require_session_and_token().await?;
    let target = session::target_cfg()?;
    let storage = session::storage()?;
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
/// a single commit, then patches the local projection. For 3.2-α this only
/// touches the markdown file + projection — provider-side mutation (issue
/// state) is deferred to the bidirectional sync pass.
#[server(TransitionWorkItem, "/api", endpoint = "transition_work_item")]
pub async fn transition_work_item(
    brain_id: String,
    new_state: WorkItemState,
) -> Result<WorkItemMutationResult, ServerFnError> {
    apply_work_item_mutation(brain_id, WorkItemMutation::State(new_state))
        .await
        .map_err(sfe)
}

/// Replace the assignees list on a work item. Same semantics as
/// `transition_work_item` (frontmatter + projection only in this slice).
#[server(AssignWorkItem, "/api", input = server_fn::codec::Json, endpoint = "assign_work_item")]
pub async fn assign_work_item(
    brain_id: String,
    assignees: Vec<String>,
) -> Result<WorkItemMutationResult, ServerFnError> {
    apply_work_item_mutation(brain_id, WorkItemMutation::Assignees(assignees))
        .await
        .map_err(sfe)
}

/// Set or clear the external binding of a work item. Pass `None` to unbind.
#[server(BindWorkItem, "/api", input = server_fn::codec::Json, endpoint = "bind_work_item")]
pub async fn bind_work_item(
    brain_id: String,
    binding: Option<ExternalWorkItemBinding>,
) -> Result<WorkItemMutationResult, ServerFnError> {
    apply_work_item_mutation(brain_id, WorkItemMutation::Binding(binding))
        .await
        .map_err(sfe)
}

#[server(ReadBrainFile, "/api", endpoint = "read_brain_file")]
pub async fn read_brain_file(path: String) -> Result<BrainFile, ServerFnError> {
    use crate::server::session;
    use brain_storage::Storage;

    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let cfg = session::target_cfg().map_err(sfe)?;
    let storage = session::storage().map_err(sfe)?;
    let (content, sha) = storage.read_file(&token, &path).await.map_err(sfe)?;

    let (body, _fm) = crate::markdown::split_frontmatter(&content);
    let rendered_html = crate::markdown::render_for_file(body, &path, &cfg);

    Ok(BrainFile {
        path,
        sha,
        content,
        rendered_html,
    })
}

#[server(
    SaveBrainFile,
    "/api",
    input = server_fn::codec::Json,
    endpoint = "save_brain_file",
)]
pub async fn save_brain_file(payload: BrainFilePayload) -> Result<WriteResult, ServerFnError> {
    use crate::server::session;

    let (s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let user = session::session_user_or_fallback(&s).await;

    let target = session::target_cfg().map_err(sfe)?;
    let config = crate::knowledge::config_loader::load(&target, &token).await;

    let file_path = match &payload.path {
        Some(p) if !p.is_empty() => p.clone(),
        _ => {
            let slug = payload
                .title
                .replace(' ', "-")
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '-')
                .collect::<String>();
            let dir = payload
                .folder
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| {
                    config
                        .lookup(&payload.node_type)
                        .map(|s| s.directory.as_str())
                        .unwrap_or("")
                })
                .trim_matches('/');
            if dir.is_empty() {
                format!("{}.md", slug)
            } else {
                format!("{}/{}.md", dir, slug)
            }
        }
    };

    let related_section = build_related_section(&file_path, &payload.related);
    let body_without_related = strip_related_section(&payload.body);

    let markdown = format!(
        "{}\n{}{}",
        merge_frontmatter(&payload, &user, &config),
        body_without_related,
        related_section,
    );

    let auto_msg = if payload.sha.is_some() {
        format!("Update {} via Brain UI", file_path)
    } else {
        format!("Create {} via Brain UI", file_path)
    };
    let commit_msg = sanitize_commit_message(payload.commit_message.as_deref()).unwrap_or(auto_msg);

    let storage = session::storage().map_err(sfe)?;
    let author_email = format!("{}@users.noreply.github.com", user);

    match save_file_permission_aware(
        &storage,
        &token,
        &file_path,
        &markdown,
        payload.sha.as_deref(),
        &commit_msg,
        &user,
        &author_email,
        &target,
    )
    .await
    {
        Ok(result) => {
            let kind = if payload.sha.is_some() {
                "update"
            } else {
                "create"
            };
            match result.mode {
                WriteMode::Direct => {
                    crate::server::audit::log(kind, Some(&user), &file_path).await;
                    rebuild_projection_after_write(
                        &storage,
                        &target,
                        &token,
                        &user,
                        &format!("write:{file_path}"),
                    )
                    .await;
                }
                WriteMode::PullRequest => {
                    crate::server::audit::log(
                        "propose_write",
                        Some(&user),
                        &format!(
                            "{} via PR #{}",
                            file_path,
                            result
                                .pr_number
                                .map(|n| n.to_string())
                                .unwrap_or_else(|| "?".to_string())
                        ),
                    )
                    .await;
                }
            }
            Ok(result)
        }
        Err(e) => {
            crate::server::audit::log("api_error", Some(&user), &format!("save {file_path}: {e}"))
                .await;
            Err(sfe(e))
        }
    }
}

#[server(DeleteBrainFile, "/api", endpoint = "delete_brain_file")]
pub async fn delete_brain_file(
    path: String,
    sha: String,
    commit_message: Option<String>,
) -> Result<WriteResult, ServerFnError> {
    use crate::server::session;

    let (s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let user = session::session_user_or_fallback(&s).await;
    let target = session::target_cfg().map_err(sfe)?;
    let author_email = format!("{}@users.noreply.github.com", user);
    let commit_msg = sanitize_commit_message(commit_message.as_deref())
        .unwrap_or_else(|| format!("Delete {} via Brain UI", path));

    let storage = session::storage().map_err(sfe)?;
    match delete_file_permission_aware(
        &storage,
        &token,
        &path,
        &sha,
        &commit_msg,
        &user,
        &author_email,
        &target,
    )
    .await
    {
        Ok(result) => {
            match result.mode {
                WriteMode::Direct => {
                    crate::server::audit::log("delete", Some(&user), &path).await;
                    rebuild_projection_after_write(
                        &storage,
                        &target,
                        &token,
                        &user,
                        &format!("delete:{path}"),
                    )
                    .await;
                }
                WriteMode::PullRequest => {
                    crate::server::audit::log(
                        "propose_delete",
                        Some(&user),
                        &format!(
                            "{} via PR #{}",
                            path,
                            result
                                .pr_number
                                .map(|n| n.to_string())
                                .unwrap_or_else(|| "?".to_string())
                        ),
                    )
                    .await;
                }
            }
            Ok(result)
        }
        Err(e) => {
            crate::server::audit::log("api_error", Some(&user), &format!("delete {path}: {e}"))
                .await;
            Err(sfe(e))
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenameResult {
    pub new_path: String,
    /// Paths of files whose links were rewritten to point at `new_path`.
    pub updated_referrers: Vec<String>,
    pub write: WriteResult,
}

/// Move a file to a new path and rewrite every markdown link that pointed at
/// the old path. Issues one commit per touched file (referrers, then the move
/// itself); we accept the commit churn to stay on the simple Contents API
/// rather than assembling a Git Data tree.
#[server(RenameBrainFile, "/api", endpoint = "rename_brain_file")]
pub async fn rename_brain_file(
    old_path: String,
    new_path: String,
    old_sha: String,
    commit_message: Option<String>,
) -> Result<RenameResult, ServerFnError> {
    use crate::server::session;

    let old_path = old_path.trim().trim_matches('/').to_string();
    let new_path = new_path.trim().trim_matches('/').to_string();

    if new_path.is_empty() || old_path.is_empty() {
        return Err(sfe(BrainError::parse("Empty path")));
    }
    if new_path == old_path {
        return Err(sfe(BrainError::parse("New path matches old path")));
    }
    if !new_path.ends_with(".md") {
        return Err(sfe(BrainError::parse("New path must end in .md")));
    }
    if new_path.contains("..") || new_path.starts_with('/') {
        return Err(sfe(BrainError::parse("Invalid new path")));
    }

    let (s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let user = session::session_user_or_fallback(&s).await;
    let author_email = format!("{}@users.noreply.github.com", user);
    let target = session::target_cfg().map_err(sfe)?;
    let storage = session::storage().map_err(sfe)?;

    let user_msg = sanitize_commit_message(commit_message.as_deref());
    let permissions = storage.repository_permissions(&token).await.map_err(sfe)?;

    if permissions.push {
        match perform_rename_on_storage(
            &storage,
            &token,
            &old_path,
            &new_path,
            &old_sha,
            user_msg.clone(),
            &user,
            &author_email,
        )
        .await
        {
            Ok(updated_referrers) => {
                crate::server::audit::log(
                    "rename",
                    Some(&user),
                    &format!(
                        "{old_path} -> {new_path} ({} referrers)",
                        updated_referrers.len()
                    ),
                )
                .await;
                rebuild_projection_after_write(
                    &storage,
                    &target,
                    &token,
                    &user,
                    &format!("rename:{old_path}->{new_path}"),
                )
                .await;
                return Ok(RenameResult {
                    new_path: new_path.clone(),
                    updated_referrers,
                    write: WriteResult::direct(new_path),
                });
            }
            Err(error) if should_fallback_to_pr(&error) => {}
            Err(error) => return Err(sfe(error)),
        }
    }

    let plan = prepare_pr_write(
        &storage,
        &token,
        &user,
        &target,
        "rename",
        &old_path,
        permissions.push,
    )
    .await
    .map_err(sfe)?;
    let updated_referrers = perform_rename_on_storage(
        &plan.storage,
        &token,
        &old_path,
        &new_path,
        &old_sha,
        user_msg,
        &user,
        &author_email,
    )
    .await
    .map_err(sfe)?;
    let pr = open_write_pr(
        &storage,
        &token,
        &plan,
        &format!("Propose rename {old_path} to {new_path} via Brain UI"),
        &format!("Brain UI could not rename `{old_path}` directly on `{}` and proposed the rename through a pull request instead.\n\nNew path: `{new_path}`\nRewritten referrers: {}", target.branch, updated_referrers.len()),
    )
    .await
    .map_err(sfe)?;
    crate::server::audit::log(
        "propose_rename",
        Some(&user),
        &format!("{old_path} -> {new_path} via PR #{}", pr.number),
    )
    .await;

    Ok(RenameResult {
        new_path: new_path.clone(),
        updated_referrers,
        write: WriteResult::pull_request(new_path, plan.branch, pr.number, pr.html_url),
    })
}

#[cfg(feature = "ssr")]
#[allow(clippy::too_many_arguments)]
async fn perform_rename_on_storage(
    storage: &brain_storage::GithubStorage,
    token: &str,
    old_path: &str,
    new_path: &str,
    old_sha: &str,
    user_msg: Option<String>,
    user: &str,
    author_email: &str,
) -> Result<Vec<String>, BrainError> {
    use brain_storage::Storage;

    // Sanity: the source file still exists at the sha the client saw.
    let (old_content, live_sha) = storage.read_file(token, old_path).await?;
    if live_sha != old_sha {
        return Err(BrainError::conflict(
            "File was modified since you opened it; reload and retry",
        ));
    }

    let config = crate::knowledge::config_loader::load(storage.target(), token).await;

    // Find every file that links to old_path. Walk the tree once, read each
    // candidate, and string-scan for link targets that resolve to old_path.
    let (_nodes, _edges) = storage.load_graph(token, &config).await?;
    let all_paths = collect_repo_md_paths(token, storage).await?;

    // Collect every referrer that needs a link rewrite together with the
    // renamed file's new path. They will be committed together via the Git
    // Data API instead of one Contents API commit per file.
    let mut upserts: Vec<(String, String)> = Vec::new();
    let mut expected_shas: Vec<(String, String)> = vec![(old_path.to_string(), live_sha.clone())];
    let mut updated_referrers = Vec::<String>::new();
    for candidate in &all_paths {
        if candidate == old_path {
            continue;
        }
        let (content, sha) = match storage.read_file(token, candidate).await {
            Ok(v) => v,
            Err(_) => continue,
        };
        let Some(rewritten) = rewrite_links(&content, candidate, old_path, new_path) else {
            continue;
        };
        upserts.push((candidate.clone(), rewritten));
        expected_shas.push((candidate.clone(), sha));
        updated_referrers.push(candidate.clone());
    }
    upserts.push((new_path.to_string(), old_content));

    let referrer_count = updated_referrers.len();
    let message = user_msg.unwrap_or_else(|| {
        if referrer_count == 0 {
            format!("Rename {old_path} -> {new_path} via Brain UI")
        } else {
            format!("Rename {old_path} -> {new_path} via Brain UI ({referrer_count} referrers)")
        }
    });

    storage
        .atomic_rename(
            token,
            brain_storage::RenameMutation {
                upserts,
                deletes: vec![old_path.to_string()],
                expect_absent: vec![new_path.to_string()],
                expected_shas,
                message,
                author_name: user.to_string(),
                author_email: author_email.to_string(),
            },
            brain_storage::BackoffPolicy::default(),
        )
        .await?;

    Ok(updated_referrers)
}

#[cfg(feature = "ssr")]
async fn collect_repo_md_paths(
    token: &str,
    storage: &brain_storage::GithubStorage,
) -> Result<Vec<String>, BrainError> {
    use brain_domain::GithubClient;
    use brain_graph::is_included_md;
    use brain_storage::GithubHttp;
    // Reuse graph load's internal logic by re-reading the tree directly. Keep
    // this narrow — we only need paths, not parsed docs. Build the URL from
    // the storage's actual target so a rename always reads the tree of the
    // repo it's modifying, never the process-default target.
    let url = GithubClient::new(storage.target().clone()).tree_url();
    #[derive(serde::Deserialize)]
    struct Tree {
        tree: Vec<Entry>,
    }
    #[derive(serde::Deserialize)]
    struct Entry {
        path: String,
        #[serde(rename = "type")]
        kind: String,
    }
    let resp: Tree = GithubHttp::send_json(storage.http().get(&url, token), "tree").await?;
    Ok(resp
        .tree
        .into_iter()
        .filter(|e| e.kind == "blob" && is_included_md(&e.path))
        .map(|e| e.path)
        .collect())
}

/// Given the content of `file_path`, rewrite any `](X)` whose X resolves to
/// `old_target` so X becomes the correct relative path to `new_target`.
/// Returns `None` if nothing changed.
#[cfg(feature = "ssr")]
fn rewrite_links(
    content: &str,
    file_path: &str,
    old_target: &str,
    new_target: &str,
) -> Option<String> {
    use std::path::Path;

    let from_dir = Path::new(file_path).parent().unwrap_or(Path::new(""));
    let new_rel = relativize(from_dir, new_target);

    let mut out = String::with_capacity(content.len());
    let mut i = 0;
    let bytes = content.as_bytes();
    let mut changed = false;
    while i < bytes.len() {
        if bytes[i] == b']'
            && i + 1 < bytes.len()
            && bytes[i + 1] == b'('
            && let Some(end) = content[i + 2..].find(')')
        {
            let url = &content[i + 2..i + 2 + end];
            let (path_part, fragment) = match url.split_once('#') {
                Some((p, f)) => (p, Some(f)),
                None => (url, None),
            };
            if !path_part.starts_with("http")
                && path_part.ends_with(".md")
                && resolve_link_path(from_dir, path_part) == old_target
            {
                out.push_str("](");
                out.push_str(&new_rel);
                if let Some(f) = fragment {
                    out.push('#');
                    out.push_str(f);
                }
                out.push(')');
                i = i + 2 + end + 1;
                changed = true;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    changed.then_some(out)
}

#[cfg(feature = "ssr")]
fn resolve_link_path(from_dir: &std::path::Path, link: &str) -> String {
    use std::path::Path;
    let joined = from_dir.join(link);
    let mut parts: Vec<&str> = Vec::new();
    for comp in Path::new(&joined).iter() {
        let Some(s) = comp.to_str() else {
            return String::new();
        };
        if s == "." {
            continue;
        } else if s == ".." {
            parts.pop();
        } else {
            parts.push(s);
        }
    }
    parts.join("/")
}

/// Shortest relative path from `from_dir` to `target` (both repo-rooted).
#[cfg(feature = "ssr")]
fn relativize(from_dir: &std::path::Path, target: &str) -> String {
    let from_parts: Vec<&str> = from_dir
        .to_str()
        .unwrap_or("")
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    let target_parts: Vec<&str> = target.split('/').filter(|s| !s.is_empty()).collect();

    let mut common = 0;
    while common < from_parts.len()
        && common < target_parts.len() - 1
        && from_parts[common] == target_parts[common]
    {
        common += 1;
    }

    let ups = from_parts.len() - common;
    let mut out = String::new();
    for _ in 0..ups {
        out.push_str("../");
    }
    if ups == 0 {
        out.push_str("./");
    }
    out.push_str(&target_parts[common..].join("/"));
    out
}

/// Remove any trailing "## Related / See also" section from the body so the
/// rebuilt section (from the picker) doesn't duplicate links already present.
#[cfg(feature = "ssr")]
fn strip_related_section(body: &str) -> &str {
    let mut last_related_start: Option<usize> = None;
    let mut search_start = 0;
    while let Some(pos) = body[search_start..].find("## ") {
        let abs = search_start + pos;
        let line_end = body[abs..]
            .find('\n')
            .map(|n| abs + n)
            .unwrap_or(body.len());
        let heading = body[abs..line_end].to_lowercase();
        if heading.contains("related") || heading.contains("see also") {
            last_related_start = Some(abs);
        }
        search_start = line_end + 1;
        if search_start >= body.len() {
            break;
        }
    }
    match last_related_start {
        Some(pos) => body[..pos].trim_end_matches('\n'),
        None => body,
    }
}

#[cfg(feature = "ssr")]
fn build_related_section(file_path: &str, related: &[String]) -> String {
    use std::path::Path;

    if related.is_empty() {
        return String::new();
    }

    let from_dir = Path::new(file_path).parent().unwrap_or(Path::new(""));
    let links: Vec<String> = related
        .iter()
        .map(|path| {
            let label = path
                .rsplit('/')
                .next()
                .unwrap_or(path)
                .trim_end_matches(".md");
            let relative = relativize(from_dir, path);
            format!("- [{}]({})", label, relative)
        })
        .collect();
    format!("\n## Related / See also\n\n{}\n", links.join("\n"))
}

/// Max size for a single asset upload. GitHub Contents API accepts larger, but
/// we keep it modest to stay responsive and avoid ballooning the repo.
#[cfg(feature = "ssr")]
const MAX_ASSET_BYTES: usize = 2 * 1024 * 1024;

#[server(
    UploadAsset,
    "/api",
    input = server_fn::codec::Json,
    endpoint = "upload_asset",
)]
pub async fn upload_asset(filename: String, bytes: Vec<u8>) -> Result<String, ServerFnError> {
    use crate::server::session;
    use brain_storage::Storage;

    if bytes.is_empty() {
        return Err(sfe(BrainError::parse("Empty upload")));
    }
    if bytes.len() > MAX_ASSET_BYTES {
        return Err(sfe(BrainError::parse(format!(
            "Upload too large ({} bytes; max {})",
            bytes.len(),
            MAX_ASSET_BYTES
        ))));
    }

    let (stem, ext) = split_filename(&filename);
    if !is_allowed_image_ext(&ext) {
        return Err(sfe(BrainError::parse(format!(
            "Unsupported file extension: .{ext}"
        ))));
    }

    let (s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let user = session::session_user_or_fallback(&s).await;
    let author_email = format!("{}@users.noreply.github.com", user);

    let today = time::OffsetDateTime::now_utc();
    let short_hash = short_content_hash(&bytes);
    let slug = slugify(&stem);
    let asset_path = format!(
        "assets/{:04}/{:02}/{}-{}.{}",
        today.year(),
        today.month() as u8,
        slug,
        short_hash,
        ext,
    );

    let commit_msg = format!("Upload {asset_path} via Brain UI");
    let storage = session::storage().map_err(sfe)?;
    match storage
        .upload_binary(
            &token,
            &asset_path,
            &bytes,
            &commit_msg,
            &user,
            &author_email,
        )
        .await
    {
        Ok(path) => {
            crate::server::audit::log("upload_asset", Some(&user), &asset_path).await;
            Ok(path)
        }
        Err(e) => {
            crate::server::audit::log(
                "api_error",
                Some(&user),
                &format!("upload_asset {asset_path}: {e}"),
            )
            .await;
            Err(sfe(e))
        }
    }
}

#[cfg(feature = "ssr")]
fn split_filename(filename: &str) -> (String, String) {
    let name = filename.rsplit('/').next().unwrap_or(filename);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem.to_string(), ext.to_lowercase()),
        _ => (name.to_string(), String::new()),
    }
}

#[cfg(feature = "ssr")]
fn is_allowed_image_ext(ext: &str) -> bool {
    matches!(ext, "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg")
}

#[cfg(feature = "ssr")]
fn slugify(stem: &str) -> String {
    let mut out = String::with_capacity(stem.len());
    let mut prev_dash = false;
    for c in stem.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "asset".to_string()
    } else if trimmed.len() > 40 {
        trimmed.chars().take(40).collect()
    } else {
        trimmed
    }
}

/// Short content-derived suffix so two uploads with the same slug don't collide.
/// Not cryptographic — just needs to be stable and short.
#[cfg(feature = "ssr")]
fn short_content_hash(bytes: &[u8]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    format!("{:x}", h.finish()).chars().take(8).collect()
}

#[cfg(feature = "ssr")]
#[allow(clippy::too_many_arguments)]
async fn save_file_permission_aware(
    storage: &brain_storage::GithubStorage,
    token: &str,
    path: &str,
    content: &str,
    sha: Option<&str>,
    message: &str,
    user: &str,
    author_email: &str,
    target: &TargetConfig,
) -> Result<WriteResult, BrainError> {
    use brain_storage::Storage;

    let permissions = storage.repository_permissions(token).await?;
    if permissions.push {
        match storage
            .save_file(token, path, content, sha, message, user, author_email)
            .await
        {
            Ok(path) => return Ok(WriteResult::direct(path)),
            Err(error) if should_fallback_to_pr(&error) => {}
            Err(error) => return Err(error),
        }
    }

    let plan =
        prepare_pr_write(storage, token, user, target, "save", path, permissions.push).await?;
    let written_path = plan
        .storage
        .save_file(token, path, content, sha, message, user, author_email)
        .await?;
    let pr = open_write_pr(
        storage,
        token,
        &plan,
        &format!("Propose {path} via Brain UI"),
        &format!("Brain UI could not write directly to `{}` and proposed this change through a pull request instead.\n\nTouched path: `{path}`", target.branch),
    )
    .await?;
    Ok(WriteResult::pull_request(
        written_path,
        plan.branch,
        pr.number,
        pr.html_url,
    ))
}

#[cfg(feature = "ssr")]
#[allow(clippy::too_many_arguments)]
async fn delete_file_permission_aware(
    storage: &brain_storage::GithubStorage,
    token: &str,
    path: &str,
    sha: &str,
    message: &str,
    user: &str,
    author_email: &str,
    target: &TargetConfig,
) -> Result<WriteResult, BrainError> {
    use brain_storage::Storage;

    let permissions = storage.repository_permissions(token).await?;
    if permissions.push {
        match storage
            .delete_file(token, path, sha, message, user, author_email)
            .await
        {
            Ok(()) => return Ok(WriteResult::direct(path)),
            Err(error) if should_fallback_to_pr(&error) => {}
            Err(error) => return Err(error),
        }
    }

    let plan = prepare_pr_write(
        storage,
        token,
        user,
        target,
        "delete",
        path,
        permissions.push,
    )
    .await?;
    plan.storage
        .delete_file(token, path, sha, message, user, author_email)
        .await?;
    let pr = open_write_pr(
        storage,
        token,
        &plan,
        &format!("Propose deleting {path} via Brain UI"),
        &format!("Brain UI could not delete `{path}` directly from `{}` and proposed the deletion through a pull request instead.", target.branch),
    )
    .await?;
    Ok(WriteResult::pull_request(
        path,
        plan.branch,
        pr.number,
        pr.html_url,
    ))
}

#[cfg(feature = "ssr")]
fn should_fallback_to_pr(error: &BrainError) -> bool {
    match error {
        BrainError::GitHub(message) => {
            message.contains("403")
                || message.to_lowercase().contains("protected")
                || message.to_lowercase().contains("resource not accessible")
        }
        _ => false,
    }
}

#[cfg(feature = "ssr")]
struct PrWritePlan {
    storage: brain_storage::GithubStorage,
    branch: String,
    head: String,
}

#[cfg(feature = "ssr")]
async fn prepare_pr_write(
    upstream_storage: &brain_storage::GithubStorage,
    token: &str,
    user: &str,
    target: &TargetConfig,
    action: &str,
    path: &str,
    can_push_upstream: bool,
) -> Result<PrWritePlan, BrainError> {
    let base_sha = upstream_storage.head_sha(token).await?;
    let branch = pr_branch_name(user, action, path);

    if can_push_upstream {
        upstream_storage
            .create_branch_from_sha(token, &branch, &base_sha)
            .await?;
        let branch_target = TargetConfig {
            org: target.org.clone(),
            repo: target.repo.clone(),
            branch: branch.clone(),
        };
        return Ok(PrWritePlan {
            storage: brain_storage::GithubStorage::new(
                upstream_storage.http().clone(),
                branch_target,
            ),
            branch: branch.clone(),
            head: branch,
        });
    }

    upstream_storage.ensure_fork(token, user).await?;
    let fork_target = TargetConfig {
        org: user.to_string(),
        repo: target.repo.clone(),
        branch: branch.clone(),
    };
    let fork_storage =
        brain_storage::GithubStorage::new(upstream_storage.http().clone(), fork_target);
    create_branch_with_retry(&fork_storage, token, &branch, &base_sha).await?;
    Ok(PrWritePlan {
        storage: fork_storage,
        branch: branch.clone(),
        head: format!("{user}:{branch}"),
    })
}

#[cfg(feature = "ssr")]
async fn create_branch_with_retry(
    storage: &brain_storage::GithubStorage,
    token: &str,
    branch: &str,
    sha: &str,
) -> Result<(), BrainError> {
    let delays = [
        std::time::Duration::from_millis(0),
        std::time::Duration::from_millis(1_000),
        std::time::Duration::from_millis(2_000),
        std::time::Duration::from_millis(4_000),
    ];
    let mut last_error = None;
    for delay in delays {
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        match storage.create_branch_from_sha(token, branch, sha).await {
            Ok(()) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| BrainError::github("branch create failed")))
}

#[cfg(feature = "ssr")]
async fn open_write_pr(
    upstream_storage: &brain_storage::GithubStorage,
    token: &str,
    plan: &PrWritePlan,
    title: &str,
    body: &str,
) -> Result<brain_storage::PullRequestOutcome, BrainError> {
    upstream_storage
        .open_pull_request(
            token,
            &plan.head,
            &upstream_storage.target().branch,
            title,
            body,
        )
        .await
}

#[cfg(feature = "ssr")]
fn pr_branch_name(user: &str, action: &str, path: &str) -> String {
    let ts = time::OffsetDateTime::now_utc().unix_timestamp();
    let user = slugify(user);
    let path = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".md");
    let path = slugify(path);
    format!("patch/{user}/{ts}-{action}-{path}")
}

#[cfg(feature = "ssr")]
async fn rebuild_projection_after_write(
    storage: &brain_storage::GithubStorage,
    target: &TargetConfig,
    token: &str,
    user: &str,
    reason: &str,
) {
    use brain_domain::TargetKey;
    let key = TargetKey::from(target);
    brain_storage::invalidate(&key);
    brain_storage::invalidate_template(&key);
    crate::knowledge::config_loader::invalidate(&key);
    let config = crate::knowledge::config_loader::load(target, token).await;
    if let Err(error) = crate::server::projection::rebuild(storage, token, &config, reason).await {
        crate::server::audit::log(
            "projection_error",
            Some(user),
            &format!("{reason}: {error}"),
        )
        .await;
    }
}

/// One mutation against a work item's editorial frontmatter. Each variant
/// targets a single YAML field so the patch stays surgical — other custom
/// frontmatter keys are preserved verbatim.
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

    /// Whether this mutation changes the external binding (vs the work item
    /// itself). Drives the choice between `BindingUpdated` and
    /// `WorkItemUpdated` SSE events.
    fn is_binding(&self) -> bool {
        matches!(self, WorkItemMutation::Binding(_))
    }
}

/// Apply a single mutation to a work item: load file → patch frontmatter →
/// commit single file → patch projection → publish SSE. For `split` and
/// `external` GitHub-bound items, the mutation is also pushed to the provider
/// after the editorial save; provider failure is audited without rolling back
/// the Brain file commit.
#[cfg(feature = "ssr")]
async fn apply_work_item_mutation(
    brain_id: String,
    mutation: WorkItemMutation,
) -> Result<WorkItemMutationResult, BrainError> {
    use crate::server::session;

    let (s, token) = session::require_session_and_token().await?;
    let user = session::session_user_or_fallback(&s).await;
    let target = session::target_cfg()?;
    let storage = session::storage()?;

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
        .ok_or_else(|| BrainError::parse(format!("work item not found: {brain_id}")))?;
    let path = current
        .content_path
        .clone()
        .ok_or_else(|| BrainError::parse(format!("work item {brain_id} has no content path")))?;
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

/// Apply provider-originated issue changes to Brain's editorial file and
/// projection without echoing them back to the provider. Used by GitHub issue
/// webhooks after the external item has already changed.
#[cfg(feature = "ssr")]
pub(crate) async fn apply_provider_work_item_update(
    token: &str,
    user: &str,
    target: &TargetConfig,
    storage: &brain_storage::GithubStorage,
    brain_id: &str,
    state: Option<WorkItemState>,
    assignees: Option<Vec<String>>,
) -> Result<Option<WorkItem>, BrainError> {
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
    target: &TargetConfig,
    storage: &brain_storage::GithubStorage,
    brain_id: String,
    mutation: WorkItemMutation,
    sync_provider: bool,
    patch_projection: bool,
    publish_event: bool,
) -> Result<WorkItem, BrainError> {
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

    // Empty frontmatter would mean either a non-work-item file or a malformed
    // document the projection somehow indexed. Either way refuse to write.
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
            map.insert("state".to_string(), serialized);
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
        // Patch the local projection so the same response can carry the fresh
        // record without waiting on a full rebuild.
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
                brain_id: brain_id.clone(),
                content_path: Some(path.clone()),
            }
        } else {
            crate::server::sse::BrainEvent::WorkItemUpdated {
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
    config: &BrainConfig,
    item: &WorkItem,
    mutation: &WorkItemMutation,
    user: &str,
) -> Result<(), BrainError> {
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
    config: &BrainConfig,
    item: &WorkItem,
    state: &WorkItemState,
) -> Result<Option<Vec<String>>, BrainError> {
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

#[server(ListBrainFolders, "/api", endpoint = "list_brain_folders")]
pub async fn list_brain_folders() -> Result<Vec<String>, ServerFnError> {
    use crate::server::session;
    use brain_storage::Storage;

    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let storage = session::storage().map_err(sfe)?;
    storage.list_folders(&token).await.map_err(sfe)
}

/// Drop the per-target in-memory caches and rebuild the local SQLite
/// projection from the forge. This is the explicit manual reindex path used
/// for drift recovery until inbound webhooks/SSE exist.
#[server(RefreshBrainGraph, "/api", endpoint = "refresh_brain_graph")]
pub async fn refresh_brain_graph() -> Result<(), ServerFnError> {
    use crate::server::session;
    use brain_domain::TargetKey;
    let (s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let user = session::session_user_or_fallback(&s).await;
    let target = session::target_cfg().map_err(sfe)?;
    let key = TargetKey::from(&target);
    brain_storage::invalidate(&key);
    brain_storage::invalidate_template(&key);
    crate::knowledge::config_loader::invalidate(&key);
    let storage = session::storage().map_err(sfe)?;
    let config = crate::knowledge::config_loader::load(&target, &token).await;
    match crate::server::projection::rebuild(&storage, &token, &config, "manual_refresh").await {
        Ok(()) => {
            crate::server::audit::log("projection_rebuild", Some(&user), key.as_str()).await;
            Ok(())
        }
        Err(error) => {
            crate::server::audit::log(
                "projection_error",
                Some(&user),
                &format!("manual_refresh {}: {}", key.as_str(), error),
            )
            .await;
            Err(sfe(error))
        }
    }
}

#[cfg(feature = "ssr")]
fn today_iso() -> String {
    let today = time::OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}",
        today.year(),
        today.month() as u8,
        today.day()
    )
}

/// Build the final frontmatter block by merging the form's authoritative
/// fields onto the document's preserved map (update) or onto a seeded
/// template (create). Preserves custom keys (status, severity, cliente,
/// etc.) that the form doesn't manage, per the fix for caveat #5.
#[cfg(feature = "ssr")]
fn merge_frontmatter(payload: &BrainFilePayload, author: &str, config: &BrainConfig) -> String {
    use rand::{Rng, distributions::Alphanumeric};
    use serde_yaml::Value;

    if config.synthetic_tag_spec().map(|s| s.name.as_str()) == Some(payload.node_type.as_str()) {
        return String::new();
    }

    let date = today_iso();
    let is_update = payload.preserved_frontmatter.is_some();
    let spec = config
        .lookup(&payload.node_type)
        .unwrap_or_else(|| config.default_spec());

    let mut map = payload
        .preserved_frontmatter
        .clone()
        .unwrap_or_else(|| spec.frontmatter_seed.clone());

    // Form-authoritative fields: always overwrite.
    map.insert("type".into(), Value::String(spec.name.clone()));
    map.insert("author".into(), Value::String(author.to_string()));
    map.insert(
        "tags".into(),
        Value::Sequence(
            payload
                .tags
                .iter()
                .map(|t| Value::String(t.clone()))
                .collect(),
        ),
    );
    // Title is controlled by the form for types that declare a title_key.
    if let Some(key) = spec.title_key.as_deref() {
        map.insert(key.into(), Value::String(payload.title.clone()));
    }

    if !is_update {
        if let Some(field) = spec.date_create_field.as_deref() {
            map.insert(field.into(), Value::String(date));
        }
    } else if let Some(field) = spec.date_update_field.as_deref() {
        map.insert(field.into(), Value::String(date));
    }

    if spec.is_work_item() {
        let needs_brain_id = map
            .get("brain_id")
            .and_then(|value| value.as_str())
            .is_none_or(|value| value.trim().is_empty());
        if needs_brain_id {
            let suffix = rand::thread_rng()
                .sample_iter(&Alphanumeric)
                .take(6)
                .map(char::from)
                .collect::<String>()
                .to_ascii_lowercase();
            let timestamp = time::OffsetDateTime::now_utc().unix_timestamp();
            map.insert(
                "brain_id".into(),
                Value::String(format!("{}-{timestamp}-{suffix}", spec.name)),
            );
        }
    }

    match serde_yaml::to_string(&map) {
        Ok(yaml) => format!("---\n{}---\n", yaml),
        Err(_) => String::new(),
    }
}

#[cfg(all(test, feature = "ssr"))]
mod merge_frontmatter_tests {
    use super::*;
    use std::collections::BTreeMap;

    fn base_payload(node_type: String) -> BrainFilePayload {
        BrainFilePayload {
            node_type,
            title: "T".into(),
            author: "alice".into(),
            tags: vec!["x".into()],
            body: String::new(),
            related: vec![],
            folder: None,
            path: Some("adrs/F.md".into()),
            sha: Some("sha".into()),
            commit_message: None,
            preserved_frontmatter: None,
            frontmatter_malformed: false,
        }
    }

    #[test]
    fn update_preserves_custom_fields() {
        let mut preserved = BTreeMap::new();
        preserved.insert(
            "status".into(),
            serde_yaml::Value::String("accepted".into()),
        );
        preserved.insert(
            "date".into(),
            serde_yaml::Value::String("2026-03-01".into()),
        );
        let mut payload = base_payload("adr".to_string());
        payload.preserved_frontmatter = Some(preserved);

        let out = merge_frontmatter(&payload, "bob", &BrainConfig::default());
        assert!(out.contains("status: accepted"), "out was: {out}");
        assert!(out.contains("date: 2026-03-01"), "out was: {out}");
        assert!(out.contains("author: bob"), "out was: {out}");
        assert!(out.contains("type: adr"), "out was: {out}");
    }

    #[test]
    fn create_seeds_defaults() {
        let payload = base_payload("adr".to_string());
        let out = merge_frontmatter(&payload, "alice", &BrainConfig::default());
        assert!(out.contains("type: adr"));
        assert!(out.contains("status: draft"));
        assert!(out.starts_with("---\n"));
        assert!(out.ends_with("---\n"));
    }

    #[test]
    fn tag_type_emits_empty() {
        let payload = base_payload("tag".to_string());
        assert_eq!(
            merge_frontmatter(&payload, "x", &BrainConfig::default()),
            ""
        );
    }

    #[test]
    fn custom_type_respects_spec_title_and_date_fields() {
        use brain_domain::NodeTypeSpec;
        let mut cfg = BrainConfig::default();
        cfg.node_types.push(NodeTypeSpec {
            name: "articolo".into(),
            label: "Articolo".into(),
            directory: "articoli".into(),
            accent: "#abcdef".into(),
            template_filename: None,
            creatable: true,
            frontmatter_seed: BTreeMap::new(),
            title_key: Some("titolo".into()),
            date_create_field: Some("creato_il".into()),
            date_update_field: Some("aggiornato_il".into()),
            body_label: Some("Corpo".into()),
            work_item_kind: None,
        });

        // Create path: title_key and date_create_field both get injected.
        let mut payload = base_payload("articolo".to_string());
        payload.title = "Il Mio Articolo".into();
        let out = merge_frontmatter(&payload, "me", &cfg);
        assert!(out.contains("titolo: Il Mio Articolo"), "out was: {out}");
        assert!(out.contains("creato_il:"), "out was: {out}");
        assert!(
            !out.contains("aggiornato_il:"),
            "update field must not appear on create: {out}"
        );

        // Update path: date_update_field is used instead.
        let mut payload = base_payload("articolo".to_string());
        payload.title = "Il Mio Articolo".into();
        payload.preserved_frontmatter = Some(BTreeMap::new());
        let out = merge_frontmatter(&payload, "me", &cfg);
        assert!(out.contains("titolo: Il Mio Articolo"));
        assert!(out.contains("aggiornato_il:"), "out was: {out}");
        assert!(
            !out.contains("creato_il:"),
            "create field must not appear on update: {out}"
        );
    }

    #[test]
    fn form_fields_win_over_preserved() {
        let mut preserved = BTreeMap::new();
        preserved.insert("author".into(), serde_yaml::Value::String("old".into()));
        preserved.insert(
            "tags".into(),
            serde_yaml::Value::Sequence(vec![serde_yaml::Value::String("stale".into())]),
        );
        let mut payload = base_payload("adr".to_string());
        payload.preserved_frontmatter = Some(preserved);
        payload.tags = vec!["new".into()];

        let out = merge_frontmatter(&payload, "bob", &BrainConfig::default());
        assert!(out.contains("author: bob"));
        assert!(out.contains("- new"));
        assert!(!out.contains("old"));
        assert!(!out.contains("stale"));
    }

    #[test]
    fn work_item_create_injects_brain_id_once() {
        use brain_domain::{NodeTypeSpec, WorkItemKind};

        let mut cfg = BrainConfig::default();
        cfg.node_types.push(NodeTypeSpec {
            name: "task".into(),
            label: "Task".into(),
            directory: "tasks".into(),
            accent: "#fb7185".into(),
            template_filename: Some("Task.md".into()),
            creatable: true,
            frontmatter_seed: BTreeMap::new(),
            title_key: Some("topic".into()),
            date_create_field: Some("date_created".into()),
            date_update_field: Some("last_updated".into()),
            body_label: Some("Description".into()),
            work_item_kind: Some(WorkItemKind::Task),
        });

        let mut payload = base_payload("task".to_string());
        payload.path = None;
        payload.sha = None;
        let out = merge_frontmatter(&payload, "alice", &cfg);
        assert!(out.contains("type: task"), "out was: {out}");
        assert!(out.contains("brain_id: task-"), "out was: {out}");
        assert!(out.contains("date_created:"), "out was: {out}");
    }

    #[test]
    fn work_item_update_preserves_existing_brain_id() {
        use brain_domain::{NodeTypeSpec, WorkItemKind};

        let mut cfg = BrainConfig::default();
        cfg.node_types.push(NodeTypeSpec {
            name: "task".into(),
            label: "Task".into(),
            directory: "tasks".into(),
            accent: "#fb7185".into(),
            template_filename: Some("Task.md".into()),
            creatable: true,
            frontmatter_seed: BTreeMap::new(),
            title_key: Some("topic".into()),
            date_create_field: Some("date_created".into()),
            date_update_field: Some("last_updated".into()),
            body_label: Some("Description".into()),
            work_item_kind: Some(WorkItemKind::Task),
        });

        let mut payload = base_payload("task".to_string());
        let mut preserved = BTreeMap::new();
        preserved.insert(
            "brain_id".into(),
            serde_yaml::Value::String("task-existing-123".into()),
        );
        payload.preserved_frontmatter = Some(preserved);
        let out = merge_frontmatter(&payload, "alice", &cfg);
        assert!(
            out.contains("brain_id: task-existing-123"),
            "out was: {out}"
        );
        assert!(
            !out.contains("brain_id: task-task-existing-123"),
            "out was: {out}"
        );
    }

    #[test]
    fn related_section_uses_relative_links_from_nested_destination() {
        let out = build_related_section(
            "concepts/sub_folder_test_brain_UI/README.md",
            &[
                "runbooks/uso-brain-ui.md".to_string(),
                "concepts/TestbrainUI.md".to_string(),
            ],
        );

        assert!(out.contains("- [uso-brain-ui](../../runbooks/uso-brain-ui.md)"));
        assert!(out.contains("- [TestbrainUI](../TestbrainUI.md)"));
    }

    #[test]
    fn related_section_uses_same_directory_relative_links() {
        let out = build_related_section(
            "runbooks/uso-brain-ui.md",
            &["runbooks/another-runbook.md".to_string()],
        );

        assert!(out.contains("- [another-runbook](./another-runbook.md)"));
    }

    #[test]
    fn strip_related_section_removes_trailing_related_block() {
        let body = "## Description\nSome content.\n\n## Related / See also\n\n- [Foo](../concepts/Foo.md)\n";
        assert_eq!(strip_related_section(body), "## Description\nSome content.");
    }

    #[test]
    fn strip_related_section_leaves_body_without_related() {
        let body = "## Description\nSome content.\n";
        assert_eq!(strip_related_section(body), body);
    }

    #[test]
    fn strip_related_section_removes_last_of_multiple_related_blocks() {
        let body = "## Related / See also\n\n- [A](../a.md)\n\n## Other\n\n## Related / See also\n\n- [B](../b.md)\n";
        let stripped = strip_related_section(body);
        assert!(!stripped.contains("- [B]"), "second block must be stripped");
        assert!(stripped.contains("## Other"), "other sections must be kept");
    }
}

/// Tests for `rewrite_links` covering the rename path. Codifies the
/// invariants that Phase 2A's "rename safety" deliverable must keep:
/// fragments preserved, non-`.md` links left alone, every prefix variant
/// (`./`, `../`, bare) handled, external URLs untouched.
#[cfg(all(test, feature = "ssr"))]
mod rewrite_links_tests {
    use super::*;

    #[test]
    fn rewrites_bare_link_in_same_directory() {
        let body = "see [old](old.md) for details";
        let out = rewrite_links(
            body,
            "concepts/host.md",
            "concepts/old.md",
            "concepts/new.md",
        );
        assert_eq!(out.as_deref(), Some("see [old](./new.md) for details"));
    }

    #[test]
    fn rewrites_dot_slash_prefixed_link() {
        let body = "see [old](./old.md)";
        let out = rewrite_links(
            body,
            "concepts/host.md",
            "concepts/old.md",
            "concepts/new.md",
        );
        assert_eq!(out.as_deref(), Some("see [old](./new.md)"));
    }

    #[test]
    fn rewrites_parent_relative_link() {
        let body = "see [x](../adrs/old.md)";
        let out = rewrite_links(body, "concepts/host.md", "adrs/old.md", "adrs/new.md");
        assert_eq!(out.as_deref(), Some("see [x](../adrs/new.md)"));
    }

    #[test]
    fn preserves_fragment_after_rename() {
        let body = "jump to [section](./old.md#deep-section)";
        let out = rewrite_links(
            body,
            "concepts/host.md",
            "concepts/old.md",
            "concepts/new.md",
        );
        assert_eq!(
            out.as_deref(),
            Some("jump to [section](./new.md#deep-section)")
        );
    }

    #[test]
    fn preserves_fragment_with_parent_relative_link() {
        let body = "[a](../sub/old.md#h2)";
        let out = rewrite_links(body, "host/x.md", "sub/old.md", "sub/new.md");
        assert_eq!(out.as_deref(), Some("[a](../sub/new.md#h2)"));
    }

    #[test]
    fn ignores_image_links_with_md_lookalike_in_alt() {
        // The link target is an image, not a markdown doc. The matcher checks
        // `.md` on `path_part`, so `image.png` must not be touched even if
        // surrounding markdown looks similar.
        let body = "![ConceptNote](./img.png) and [doc](./old.md)";
        let out = rewrite_links(
            body,
            "concepts/host.md",
            "concepts/old.md",
            "concepts/new.md",
        );
        let updated = out.expect("at least the .md link should rewrite");
        assert!(
            updated.contains("![ConceptNote](./img.png)"),
            "image untouched: {updated}"
        );
        assert!(
            updated.contains("[doc](./new.md)"),
            "doc rewritten: {updated}"
        );
    }

    #[test]
    fn leaves_external_http_links_alone() {
        let body = "[home](https://example.com/old.md)";
        let out = rewrite_links(
            body,
            "concepts/host.md",
            "concepts/old.md",
            "concepts/new.md",
        );
        assert!(out.is_none(), "external URL must not match");
    }

    #[test]
    fn returns_none_when_no_link_matches() {
        let body = "[other](./other.md)";
        let out = rewrite_links(
            body,
            "concepts/host.md",
            "concepts/old.md",
            "concepts/new.md",
        );
        assert!(out.is_none(), "non-matching link returns None");
    }

    #[test]
    fn rewrites_nested_to_nested_across_depths() {
        // Host file in `notes/x.md` links to `concepts/a.md`; rename target
        // lives at `adrs/deep/b.md`. Output must use the correct relative
        // path from the host's directory.
        let body = "[a](../concepts/a.md)";
        let out = rewrite_links(body, "notes/x.md", "concepts/a.md", "adrs/deep/b.md");
        assert_eq!(out.as_deref(), Some("[a](../adrs/deep/b.md)"));
    }

    #[test]
    fn case_only_rename_is_treated_as_distinct_path() {
        // GitHub Contents API is case-sensitive; a rename Foo.md -> foo.md
        // must rewrite links pointing at `Foo.md`. The match is exact-string
        // on the resolved path, so this confirms case-sensitivity isn't
        // accidentally normalised away.
        let body = "[x](./Foo.md)";
        let out = rewrite_links(body, "host.md", "Foo.md", "foo.md");
        assert_eq!(out.as_deref(), Some("[x](./foo.md)"));

        // Inverse: a link to `foo.md` must NOT be rewritten when only `Foo.md`
        // was renamed.
        let body = "[x](./foo.md)";
        let out = rewrite_links(body, "host.md", "Foo.md", "foo.md");
        assert!(out.is_none(), "case-different link must not match");
    }

    #[test]
    fn rewrites_multiple_links_in_one_document() {
        let body = "first [a](./old.md) then [b](./old.md#anchor) and [c](./other.md)";
        let out = rewrite_links(body, "host.md", "old.md", "new.md");
        assert_eq!(
            out.as_deref(),
            Some("first [a](./new.md) then [b](./new.md#anchor) and [c](./other.md)")
        );
    }

    #[test]
    fn rewrites_into_new_directory() {
        // Renaming into a previously-nonexistent directory must produce a valid
        // relative path (the `save_file` Contents API call creates intermediate
        // dirs; rewrite_links itself just needs to emit the right string).
        let body = "[x](./old.md)";
        let out = rewrite_links(body, "host.md", "old.md", "fresh/new.md");
        assert_eq!(out.as_deref(), Some("[x](./fresh/new.md)"));
    }

    #[test]
    fn does_not_rewrite_link_pointing_at_host_file_itself() {
        // A self-referential link (e.g. a doc linking back to its own anchor
        // via `[x](./host.md#section)`) should not match a rename of a
        // different file.
        let body = "[self](./host.md#top)";
        let out = rewrite_links(
            body,
            "concepts/host.md",
            "concepts/old.md",
            "concepts/new.md",
        );
        assert!(out.is_none());
    }
}

/// One entry in the Brain Switcher's target list.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AccessibleTarget {
    pub org: String,
    pub repo: String,
    /// Whether `.brain-config.yml` was found at the repo root.
    pub has_brain_config: bool,
}

/// Discover repos accessible to the current user that might host a Brain
/// knowledge base. Queries `GET /user/repos` (up to 100, sorted by pushed)
/// then checks each for `.brain-config.yml` existence through the Contents API.
/// Repos where the user has no read access are filtered out by the GitHub API
/// automatically.
///
/// This is intentionally best-effort: a failed per-repo config check is
/// recorded as `has_brain_config: false` rather than bubbling an error.
#[server(ListAccessibleTargets, "/api", endpoint = "list_accessible_targets")]
pub async fn list_accessible_targets() -> Result<Vec<AccessibleTarget>, ServerFnError> {
    use crate::server::session;

    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let http = session::github_http().map_err(sfe)?;

    // Fetch up to 100 repos the user can access, sorted by most-recently pushed.
    let repos_url = "https://api.github.com/user/repos?per_page=100&sort=pushed&affiliation=owner,collaborator,organization_member";
    #[derive(serde::Deserialize)]
    struct GhRepo {
        full_name: String,
        default_branch: String,
        #[serde(default)]
        archived: bool,
    }

    let repos: Vec<GhRepo> = http
        .get(repos_url, &token)
        .send()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .json()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // Fan out the per-repo config probe concurrently. With up to 100 repos at
    // ~100ms each, a sequential scan would block the Brain Switcher open for
    // ~10s. JoinSet gives us bounded concurrency via reqwest's connection pool.
    let mut set = tokio::task::JoinSet::new();
    for r in repos.into_iter().filter(|r| !r.archived) {
        let parts: Vec<&str> = r.full_name.splitn(2, '/').collect();
        if parts.len() != 2 {
            continue;
        }
        let (org, repo) = (parts[0].to_string(), parts[1].to_string());
        let target = TargetConfig {
            org: org.clone(),
            repo: repo.clone(),
            branch: r.default_branch,
        };
        let config_url = format!(
            "{}?ref={}",
            brain_domain::GithubClient::new(target.clone()).contents_url(".brain-config.yml"),
            target.branch
        );
        let http = http.clone();
        let token = token.clone();
        set.spawn(async move {
            let has_brain_config = check_brain_config_exists(&http, &token, &config_url).await;
            AccessibleTarget {
                org,
                repo,
                has_brain_config,
            }
        });
    }

    let mut targets: Vec<AccessibleTarget> = Vec::with_capacity(set.len());
    while let Some(res) = set.join_next().await {
        if let Ok(t) = res {
            targets.push(t);
        }
    }
    // Preserve a stable, predictable order regardless of probe completion order.
    targets.sort_by(|a, b| a.org.cmp(&b.org).then_with(|| a.repo.cmp(&b.repo)));
    Ok(targets)
}

/// Probe `.brain-config.yml` through the authenticated GitHub Contents API.
/// This works for private repos, unlike raw.githubusercontent.com where bearer
/// token behavior is inconsistent.
#[cfg(feature = "ssr")]
async fn check_brain_config_exists(
    http: &brain_storage::GithubHttp,
    token: &str,
    url: &str,
) -> bool {
    http.get(url, token)
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Load the brain graph for an explicit `org/repo` target (used by the
/// multi-tenant `/{org}/{repo}/knowledge` route). The `branch` parameter
/// defaults to the server-configured branch when empty.
#[server(
    LoadBrainGraphForTarget,
    "/api",
    endpoint = "load_brain_graph_for_target"
)]
pub async fn load_brain_graph_for_target(
    org: String,
    repo: String,
    branch: String,
) -> Result<(Vec<Node>, Vec<Edge>), ServerFnError> {
    use crate::server::session;
    use brain_domain::TargetConfig;

    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let fallback = session::target_cfg().map_err(sfe)?;
    let resolved_branch = if branch.is_empty() {
        fallback.branch.clone()
    } else {
        branch
    };
    let target = TargetConfig {
        org,
        repo,
        branch: resolved_branch,
    };
    let storage = session::storage_for(target.clone()).map_err(sfe)?;
    let config = crate::knowledge::config_loader::load(&target, &token).await;
    crate::server::projection::load_graph(&storage, &token, &config)
        .await
        .map_err(sfe)
}

/// Load the brain config for an explicit `org/repo` target.
#[server(
    LoadBrainConfigForTarget,
    "/api",
    endpoint = "load_brain_config_for_target"
)]
pub async fn load_brain_config_for_target(
    org: String,
    repo: String,
    branch: String,
) -> Result<BrainConfig, ServerFnError> {
    use crate::server::session;
    use brain_domain::TargetConfig;

    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let fallback = session::target_cfg().map_err(sfe)?;
    let resolved_branch = if branch.is_empty() {
        fallback.branch.clone()
    } else {
        branch
    };
    let target = TargetConfig {
        org,
        repo,
        branch: resolved_branch,
    };
    let cfg = crate::knowledge::config_loader::load(&target, &token).await;
    Ok((*cfg).clone())
}

/// Regression guard for caveat #9: `lto = true` strips Leptos's
/// `inventory::submit!` entries, so every `#[server]` fn must be listed in
/// `SERVER_FNS` and registered explicitly. Without this test, adding a server
/// fn without registering it silently 404s in release builds.
#[cfg(all(test, feature = "ssr"))]
mod server_fn_registration_tests {
    use super::SERVER_FNS;

    /// Source of `api.rs` at build time. Embedding it here keeps the test
    /// independent of the crate's filesystem layout.
    const API_SRC: &str = include_str!("api.rs");

    /// Pull the struct name out of `#[server(Name, ...)]` or `#[server(\n    Name,\n ...`).
    fn extract_server_fn_names(src: &str) -> Vec<String> {
        let mut names = Vec::new();
        let needle = "#[server(";
        for (idx, _) in src.match_indices(needle) {
            // Skip occurrences that are inside a string literal in this very
            // file (the `let needle = "#[server(";` line above) by requiring
            // the match to be at the start of a line modulo whitespace.
            let line_start = src[..idx].rfind('\n').map(|n| n + 1).unwrap_or(0);
            let prefix = &src[line_start..idx];
            if !prefix.chars().all(|c| c.is_whitespace()) {
                continue;
            }
            let after = &src[idx + needle.len()..];
            // Skip whitespace and commas; first ident is the struct name.
            let trimmed = after.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
            let name: String = trimmed
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                names.push(name);
            }
        }
        names
    }

    #[test]
    fn server_fns_registered_match_attributes() {
        let mut found = extract_server_fn_names(API_SRC);
        found.sort();
        found.dedup();

        let mut declared: Vec<String> = SERVER_FNS.iter().map(|s| (*s).to_string()).collect();
        declared.sort();
        declared.dedup();

        assert_eq!(
            found, declared,
            "every #[server(...)] fn in api.rs must appear in SERVER_FNS \
             (and register_server_functions). Found in source: {found:?}; \
             declared in SERVER_FNS: {declared:?}"
        );
    }

    #[test]
    fn extract_server_fn_names_ignores_string_literal_occurrences() {
        // Sanity: the literal needle in this test file should not be picked up
        // because it's not at start-of-line.
        let sample = "fn x() { let needle = \"#[server(Bogus,\"; }";
        let names = extract_server_fn_names(sample);
        assert!(
            names.is_empty(),
            "string-literal #[server( must be ignored: {names:?}"
        );
    }

    #[test]
    fn extract_server_fn_names_finds_real_attributes() {
        let sample = "#[server(Foo, \"/api\")]\npub async fn foo() {}\n\
                       #[server(\n    Bar,\n    \"/api\",\n)]\npub async fn bar() {}\n";
        let mut names = extract_server_fn_names(sample);
        names.sort();
        assert_eq!(names, vec!["Bar".to_string(), "Foo".to_string()]);
    }
}
