use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::knowledge::types::{Edge, Node};
use brain_domain::{BrainConfig, TargetRef};

#[cfg(feature = "ssr")]
use super::sfe;
#[cfg(feature = "ssr")]
use brain_domain::TargetConfig;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_prefix: Option<String>,
    #[serde(default = "default_include_virtual")]
    pub include_virtual: bool,
}

impl Default for NodeQueryFilters {
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

/// Parametric read side for graph nodes. This intentionally exposes the
/// target-scoped SQLite projection instead of causing a forge read; explicit
/// reconciliation stays behind `RefreshBrainGraph`, webhook handling, and
/// post-write rebuilds.
#[server(ListNodes, "/api", input = server_fn::codec::Json, endpoint = "list_nodes")]
pub async fn list_nodes(
    target: TargetRef,
    filters: NodeQueryFilters,
) -> Result<Vec<Node>, ServerFnError> {
    use crate::server::session;

    let _ = session::require_authenticated().await.map_err(sfe)?;
    let target = super::target_from_ref(target).map_err(sfe)?;
    crate::server::projection::list_nodes(
        &target,
        &crate::server::projection::NodeFilters {
            node_types: filters.node_types,
            tags: filters.tags,
            paths: filters.paths,
            path_prefix: filters.path_prefix,
            include_virtual: filters.include_virtual,
        },
    )
    .await
    .map_err(sfe)
}

/// Read one projected node by repo-relative path, without fetching file
/// content from GitHub. `ReadBrainFile` remains the markdown-content path.
#[server(ReadNode, "/api", endpoint = "read_node")]
pub async fn read_node(target: TargetRef, path: String) -> Result<Option<Node>, ServerFnError> {
    use crate::server::session;

    let _ = session::require_authenticated().await.map_err(sfe)?;
    let target = super::target_from_ref(target).map_err(sfe)?;
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

/// Drop the per-target in-memory caches and rebuild the local SQLite
/// projection from the forge. This is the explicit manual reindex path used
/// for drift recovery until inbound webhooks/SSE exist.
#[server(RefreshBrainGraph, "/api", endpoint = "refresh_brain_graph")]
pub async fn refresh_brain_graph(target: TargetRef) -> Result<(), ServerFnError> {
    use crate::server::session;
    use brain_domain::TargetKey;
    let (s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let user = session::session_user_or_fallback(&s).await;
    let target = super::target_from_ref(target).map_err(sfe)?;
    let key = TargetKey::from(&target);
    brain_storage::invalidate(&key);
    brain_storage::invalidate_template(&key);
    crate::knowledge::config_loader::invalidate(&key);
    let storage = session::storage_for(target.clone()).map_err(sfe)?;
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

/// One entry in the Brain Switcher's target list.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AccessibleTarget {
    pub org: String,
    pub repo: String,
    pub default_branch: String,
    pub active_branch: String,
    pub state: AccessibleTargetState,
    /// Whether `.brain-config.yml` was found at the repo root.
    pub has_brain_config: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccessibleTargetState {
    Accessible,
    MissingConfig,
    Forbidden,
    BranchMissing,
    ConfigInvalid,
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

    let (s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let user = session::session_user_or_fallback(&s).await;
    let http = session::github_http().map_err(sfe)?;
    let target = session::target_cfg().map_err(sfe)?;

    // Fetch up to 100 repos the user can access, sorted by most-recently pushed.
    let repos_url = brain_domain::GithubClient::new(target).user_repos_url();
    #[derive(serde::Deserialize)]
    struct GhRepo {
        full_name: String,
        default_branch: String,
        #[serde(default)]
        archived: bool,
    }

    let repos: Vec<GhRepo> = http
        .get(&repos_url, &token)
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
        if let Some(pool) = crate::server::projection::pool_handle()
            && let Err(error) = crate::server::target_registry::remember_default_branch(
                pool,
                &org,
                &repo,
                &target.branch,
                Some(&user),
            )
            .await
        {
            tracing::debug!(%error, %org, %repo, "failed to persist discovered default_branch");
        }
        let active_branch = match crate::server::projection::pool_handle() {
            Some(pool) => crate::server::target_registry::lookup(pool, &org, &repo)
                .await
                .ok()
                .flatten()
                .map(|entry| entry.branch)
                .unwrap_or_else(|| target.branch.clone()),
            None => target.branch.clone(),
        };
        let active_target = TargetConfig {
            org: target.org.clone(),
            repo: target.repo.clone(),
            branch: active_branch.clone(),
        };
        let config_url = format!(
            "{}?ref={}",
            brain_domain::GithubClient::new(active_target.clone())
                .contents_url(".brain-config.yml"),
            active_target.branch
        );
        let branch_url = brain_domain::GithubClient::new(active_target.clone())
            .branch_url(&active_target.branch);
        let http = http.clone();
        let token = token.clone();
        set.spawn(async move {
            let state = probe_brain_config(&http, &token, &config_url, &branch_url).await;
            AccessibleTarget {
                org,
                repo,
                default_branch: target.branch,
                active_branch,
                has_brain_config: state == AccessibleTargetState::Accessible,
                state,
            }
        });
    }

    let mut targets: Vec<AccessibleTarget> = Vec::with_capacity(set.len());
    while let Some(res) = set.join_next().await {
        if let Ok(t) = res {
            targets.push(t);
        }
    }
    // Drop repos with no Brain presence — they add noise to the switcher and
    // are not actionable. ConfigInvalid is kept so the user can see a broken config.
    targets.retain(|t| {
        matches!(
            t.state,
            AccessibleTargetState::Accessible | AccessibleTargetState::ConfigInvalid
        )
    });
    // Preserve a stable, predictable order regardless of probe completion order.
    targets.sort_by(|a, b| a.org.cmp(&b.org).then_with(|| a.repo.cmp(&b.repo)));
    Ok(targets)
}

/// Probe `.brain-config.yml` through the authenticated GitHub Contents API.
/// This works for private repos, unlike raw.githubusercontent.com where bearer
/// token behavior is inconsistent.
#[cfg(feature = "ssr")]
async fn probe_brain_config(
    http: &brain_storage::GithubHttp,
    token: &str,
    config_url: &str,
    branch_url: &str,
) -> AccessibleTargetState {
    let Ok(response) = http.get(config_url, token).send().await else {
        return AccessibleTargetState::Forbidden;
    };
    let status = response.status();
    if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::UNAUTHORIZED {
        return AccessibleTargetState::Forbidden;
    }
    if status == reqwest::StatusCode::NOT_FOUND {
        return if branch_exists(http, token, branch_url).await {
            AccessibleTargetState::MissingConfig
        } else {
            AccessibleTargetState::BranchMissing
        };
    }
    if !status.is_success() {
        return AccessibleTargetState::Forbidden;
    }
    #[derive(serde::Deserialize)]
    struct ContentsResponse {
        content: String,
        #[serde(default)]
        encoding: Option<String>,
    }
    let Ok(body) = response.json::<ContentsResponse>().await else {
        return AccessibleTargetState::ConfigInvalid;
    };
    if body.encoding.as_deref() != Some("base64") {
        return AccessibleTargetState::ConfigInvalid;
    }
    use base64::Engine;
    let compact: String = body
        .content
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(compact) else {
        return AccessibleTargetState::ConfigInvalid;
    };
    let Ok(raw) = String::from_utf8(decoded) else {
        return AccessibleTargetState::ConfigInvalid;
    };
    match BrainConfig::parse(&raw) {
        Ok(_) => AccessibleTargetState::Accessible,
        Err(_) => AccessibleTargetState::ConfigInvalid,
    }
}

#[cfg(feature = "ssr")]
async fn branch_exists(http: &brain_storage::GithubHttp, token: &str, url: &str) -> bool {
    http.get(url, token)
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

#[cfg(feature = "ssr")]
fn validate_legacy_target_parts(org: &str, repo: &str) -> Result<(), ServerFnError> {
    TargetRef::new(org, repo, "_")
        .validate()
        .map_err(|e| ServerFnError::new(format!("invalid target: {e}")))
}

#[server(ResolveLegacyTarget, "/api", endpoint = "resolve_legacy_target")]
pub async fn resolve_legacy_target(org: String, repo: String) -> Result<TargetRef, ServerFnError> {
    use crate::server::session;

    validate_legacy_target_parts(&org, &repo)?;
    let (s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let user = session::session_user_or_fallback(&s).await;
    let http = session::github_http().map_err(sfe)?;
    let pool = crate::server::projection::pool_handle()
        .ok_or_else(|| ServerFnError::new("Projection SQLite pool not initialized"))?;
    let entry = crate::server::target_registry::register_or_get(
        pool,
        &http,
        &token,
        &org,
        &repo,
        Some(&user),
    )
    .await
    .map_err(sfe)?;
    let target = entry.target_ref();
    target
        .validate()
        .map_err(|e| ServerFnError::new(format!("invalid target: {e}")))?;
    Ok(target)
}

/// Load the brain graph for an explicit canonical target.
#[server(
    LoadBrainGraphForTarget,
    "/api",
    endpoint = "load_brain_graph_for_target"
)]
pub async fn load_brain_graph_for_target(
    target: TargetRef,
) -> Result<(Vec<Node>, Vec<Edge>), ServerFnError> {
    use crate::server::session;

    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let target = super::target_from_ref(target).map_err(sfe)?;
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
pub async fn load_brain_config_for_target(target: TargetRef) -> Result<BrainConfig, ServerFnError> {
    use crate::server::session;

    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let target = super::target_from_ref(target).map_err(sfe)?;
    let cfg = crate::knowledge::config_loader::load(&target, &token).await;
    Ok((*cfg).clone())
}
