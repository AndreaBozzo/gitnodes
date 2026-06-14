// Copyright (C) 2026 Andrea Bozzo
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use gitnodes_domain::{BrainConfig, BrandConfig, TargetConfig, TargetRef, ViewSpec};

use super::ApiError;
#[cfg(feature = "ssr")]
use super::WriteMode;
use super::WriteResult;
#[cfg(feature = "ssr")]
use super::sfe;
#[cfg(feature = "ssr")]
use super::write_orchestrator::{rebuild_projection_after_write, save_file_permission_aware};
#[cfg(feature = "ssr")]
use gitnodes_domain::BrainError;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    pub target: TargetConfig,
    pub brand: BrandConfig,
    /// Organization required at login. `None` means org-less login; target
    /// repository permissions remain authoritative after authentication.
    pub login_org: Option<String>,
    /// True when serving a read-only local working tree (`gitnodes preview`).
    /// The landing page sends visitors straight to the graph in this mode.
    #[serde(default)]
    pub local_preview: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigLoadDiagnostic {
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigLoadStatus {
    pub config: BrainConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<ConfigLoadDiagnostic>,
}

#[cfg(feature = "ssr")]
fn config_load_status(
    snapshot: crate::knowledge::config_loader::ConfigLoadSnapshot,
) -> ConfigLoadStatus {
    ConfigLoadStatus {
        config: (*snapshot.config).clone(),
        diagnostic: snapshot.diagnostic.map(|diagnostic| ConfigLoadDiagnostic {
            message: diagnostic.message,
        }),
    }
}

// Public on purpose: the landing page (rendered for anonymous visitors)
// uses `brand.name`, `brand.org_label`, and the active `target` to advertise
// the deployment ("Sign in with GitHub to open the {org} workspace"). Org +
// repo + branch is the same info that would be in a deployment's public
// README/marketing — no auth gate. Anything truly sensitive belongs in a
// separate, gated server fn.
#[server(GetAppConfig, "/api", endpoint = "get_app_config")]
pub async fn get_app_config() -> Result<AppConfig, ApiError> {
    use crate::server::session;
    let target = session::target_cfg().map_err(sfe)?;
    let brand = use_context::<BrandConfig>()
        .ok_or_else(|| sfe(BrainError::other("No brand config available")))?;
    let login_org = crate::server::auth::login_org();
    Ok(AppConfig {
        target,
        brand,
        login_org,
        local_preview: crate::server::local::is_enabled(),
    })
}

#[server(LoadBrainConfig, "/api", endpoint = "load_brain_config")]
pub async fn load_brain_config() -> Result<BrainConfig, ApiError> {
    use crate::knowledge::config_loader;
    use crate::server::session;
    let (_s, token, target, _permissions) =
        session::require_current_target_read().await.map_err(sfe)?;
    let cfg = config_loader::load(&target, &token).await;
    Ok((*cfg).clone())
}

#[server(LoadBrainConfigStatus, "/api", endpoint = "load_brain_config_status")]
pub async fn load_brain_config_status() -> Result<ConfigLoadStatus, ApiError> {
    use crate::knowledge::config_loader;
    use crate::server::session;

    let (_s, token, target, _permissions) =
        session::require_current_target_read().await.map_err(sfe)?;
    let snapshot = config_loader::load_with_diagnostic(&target, &token).await;
    Ok(config_load_status(snapshot))
}

#[server(
    LoadBrainConfigStatusForTarget,
    "/api",
    endpoint = "load_brain_config_status_for_target"
)]
pub async fn load_brain_config_status_for_target(
    target: TargetRef,
) -> Result<ConfigLoadStatus, ApiError> {
    use crate::knowledge::config_loader;
    use crate::server::session;

    let target = super::target_from_ref(target).map_err(sfe)?;
    let (_s, token, _permissions) = session::require_target_read(&target).await.map_err(sfe)?;
    let snapshot = config_loader::load_with_diagnostic(&target, &token).await;
    Ok(config_load_status(snapshot))
}

/// Read-only list of saved views for the active target. Backed by the same
/// cached `BrainConfig` as the rest of the runtime, so it reflects the latest
/// committed state of `.gitnodes.yml` without an extra fetch.
#[server(ListViews, "/api", endpoint = "list_views")]
pub async fn list_views(target: TargetRef) -> Result<Vec<ViewSpec>, ApiError> {
    use crate::knowledge::config_loader;
    use crate::server::session;
    let target = super::target_from_ref(target).map_err(sfe)?;
    let (_s, token, _permissions) = session::require_target_read(&target).await.map_err(sfe)?;
    let cfg = config_loader::load(&target, &token).await;
    Ok(cfg.views.clone())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ViewsPreview {
    pub path: String,
    pub operation: String,
    pub current_yaml: String,
    pub proposed_yaml: String,
    pub expected_sha: Option<String>,
    pub head_sha: String,
    pub base_tree_sha: String,
}

#[server(PreviewViews, "/api", endpoint = "preview_views")]
pub async fn preview_views(
    target: TargetRef,
    views: Vec<ViewSpec>,
) -> Result<ViewsPreview, ApiError> {
    use crate::server::session;

    let target = super::target_from_ref(target).map_err(sfe)?;
    let (s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let user = session::session_user_or_fallback(&s).await;
    let storage = session::storage_for(target).map_err(sfe)?;
    crate::server::access::require_admin(&storage, &token)
        .await
        .map_err(sfe)?;

    let (current_yaml, expected_sha) = read_views_config(&storage, &token).await.map_err(sfe)?;
    let (_cfg, proposed_yaml) = build_views_config(&current_yaml, views).map_err(sfe)?;
    let transaction = views_transaction(
        &proposed_yaml,
        expected_sha.as_deref(),
        &user,
        &format!("{user}@users.noreply.github.com"),
    );
    let plan = storage
        .plan_transaction(&token, &transaction)
        .await
        .map_err(sfe)?;
    if !plan.can_commit() {
        let failed = plan
            .preconditions
            .into_iter()
            .find(|check| {
                !matches!(
                    check.status,
                    gitnodes_storage::PreconditionStatus::Satisfied
                )
            })
            .expect("can_commit false requires a failed precondition");
        if let gitnodes_storage::PreconditionStatus::Failed { kind, message } = failed.status {
            return Err(sfe(BrainError::conflict(kind, message)));
        }
    }

    Ok(ViewsPreview {
        path: CONFIG_PATH.to_string(),
        operation: if expected_sha.is_some() {
            "update".to_string()
        } else {
            "create".to_string()
        },
        current_yaml,
        proposed_yaml,
        expected_sha,
        head_sha: plan.head_sha,
        base_tree_sha: plan.base_tree_sha,
    })
}

/// Replace the entire `views` block in `.gitnodes.yml` with the supplied
/// list. Other config fields (node_types, label_taxonomy, default_type) are
/// preserved by parsing -> mutating -> re-serializing the existing file. Routes
/// through the same permission-aware orchestrator as document saves: direct
/// commit when possible, PR fallback otherwise.
///
/// Returns the same `WriteResult` shape as `SaveBrainFile` so the admin UI can
/// render `Saved` / `Proposed via PR #...` consistently with the editor.
#[server(SaveViews, "/api", endpoint = "save_views")]
pub async fn save_views(
    target: TargetRef,
    views: Vec<ViewSpec>,
    expected_sha: Option<String>,
) -> Result<WriteResult, ApiError> {
    use crate::knowledge::config_loader;
    use crate::server::session;

    let target = super::target_from_ref(target).map_err(sfe)?;
    let (s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let user = session::session_user_or_fallback(&s).await;
    let storage = session::storage_for(target.clone()).map_err(sfe)?;
    crate::server::access::require_admin(&storage, &token)
        .await
        .map_err(sfe)?;

    let (existing_raw, live_sha) = read_views_config(&storage, &token).await.map_err(sfe)?;
    ensure_preview_sha(expected_sha.as_deref(), live_sha.as_deref()).map_err(sfe)?;
    let (cfg, new_yaml) = build_views_config(&existing_raw, views).map_err(sfe)?;
    let author_email = format!("{}@users.noreply.github.com", user);
    let commit_msg = "Update saved views via Brain UI".to_string();

    match save_file_permission_aware(
        &storage,
        &token,
        CONFIG_PATH,
        &new_yaml,
        expected_sha.as_deref(),
        &commit_msg,
        &user,
        &author_email,
        &target,
        gitnodes_domain::WriteIntent::Direct,
    )
    .await
    {
        Ok(result) => {
            match result.mode {
                WriteMode::Direct => {
                    crate::server::audit::log("update_views", Some(&user), CONFIG_PATH).await;
                    rebuild_projection_after_write(
                        &storage,
                        &target,
                        &token,
                        &user,
                        "update_views",
                    )
                    .await;
                    // Seed the loader cache with the canonical post-write config
                    // *after* the projection rebuild, since rebuild calls
                    // `config_loader::invalidate` + `load`, which races GitHub's
                    // eventually-consistent contents API and could otherwise
                    // pin the pre-write view list for the 30s TTL.
                    config_loader::store(&(&target).into(), cfg);
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

#[cfg(feature = "ssr")]
const CONFIG_PATH: &str = ".gitnodes.yml";

#[cfg(feature = "ssr")]
async fn read_views_config(
    storage: &gitnodes_storage::GithubStorage,
    token: &str,
) -> Result<(String, Option<String>), BrainError> {
    use gitnodes_storage::Storage;

    match storage.read_file(token, CONFIG_PATH).await {
        Ok((raw, sha)) => Ok((raw, Some(sha))),
        Err(BrainError::NotFound(_)) => Ok((String::new(), None)),
        // `read_file` surfaces a missing file as a `content status 404` from the
        // Contents endpoint. Match that context specifically so unrelated 404s
        // (repo/permission errors) keep surfacing instead of masquerading as an
        // empty config.
        Err(error) if error.to_string().contains("content status 404") => Ok((String::new(), None)),
        Err(error) => Err(error),
    }
}

#[cfg(feature = "ssr")]
fn build_views_config(
    existing_raw: &str,
    views: Vec<ViewSpec>,
) -> Result<(BrainConfig, String), BrainError> {
    let mut cfg = if existing_raw.trim().is_empty() {
        BrainConfig::default()
    } else {
        BrainConfig::parse(existing_raw).map_err(|error| {
            BrainError::other(format!("current .gitnodes.yml does not parse: {error}"))
        })?
    };
    cfg.views = views;
    cfg.validate()
        .map_err(|error| BrainError::other(error.to_string()))?;
    let yaml = serde_yaml::to_string(&cfg)
        .map_err(|error| BrainError::other(format!("yaml serialize: {error}")))?;
    super::limits::check_len("Views config", &yaml, super::limits::MAX_VIEWS_BYTES)?;
    Ok((cfg, yaml))
}

#[cfg(feature = "ssr")]
fn views_transaction(
    yaml: &str,
    expected_sha: Option<&str>,
    user: &str,
    author_email: &str,
) -> gitnodes_storage::GitTransaction {
    let transaction = gitnodes_storage::GitTransaction::new(
        "Update saved views via Brain UI",
        user,
        author_email,
    )
    .upsert_text(CONFIG_PATH, yaml);
    match expected_sha {
        Some(sha) => transaction.expect_sha(CONFIG_PATH, sha),
        None => transaction.expect_absent(CONFIG_PATH),
    }
}

#[cfg(feature = "ssr")]
fn ensure_preview_sha(expected: Option<&str>, live: Option<&str>) -> Result<(), BrainError> {
    if expected == live {
        return Ok(());
    }
    Err(BrainError::conflict(
        gitnodes_domain::ConflictKind::BlobShaMoved,
        format!(
            "{CONFIG_PATH} changed after preview (expected {}, found {})",
            expected.unwrap_or("absent"),
            live.unwrap_or("absent")
        ),
    ))
}

#[cfg(all(test, feature = "ssr"))]
mod views_transaction_tests {
    use super::*;

    #[test]
    fn views_yaml_generation_is_deterministic() {
        let views = vec![ViewSpec {
            name: "Open tasks".into(),
            slug: "open-tasks".into(),
            tags: vec!["open".into()],
            types: Vec::new(),
            weight: Some(10),
        }];
        let (_, first) = build_views_config("", views.clone()).expect("first");
        let (_, second) = build_views_config("", views).expect("second");
        assert_eq!(first, second);
    }

    #[test]
    fn stale_preview_sha_is_rejected() {
        let error = ensure_preview_sha(Some("OLD"), Some("NEW")).expect_err("stale");
        assert!(matches!(
            error,
            BrainError::Conflict {
                kind: gitnodes_domain::ConflictKind::BlobShaMoved,
                ..
            }
        ));
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

/// Per-target projection status for the admin status surface (Schema v2).
/// Mirrors the row shape stored in `projection_sync_state` plus the target
/// identity. `webhook_lag_seconds` and `rate_limit_remaining` are surfaced
/// at the wrapper level (see `ProjectionStatus`) and intentionally left as
/// `None` placeholders this slice — they'll be wired in a follow-up.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectionStatusEntry {
    pub org: String,
    pub repo: String,
    pub branch: String,
    pub status: String,
    pub last_attempt_at: Option<String>,
    pub last_success_at: Option<String>,
    pub last_error_at: Option<String>,
    pub last_error: Option<String>,
    pub last_reason: Option<String>,
    pub file_count: i64,
    pub node_count: i64,
    pub edge_count: i64,
    pub work_item_count: i64,
    pub last_rebuild_duration_ms: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectionStatus {
    pub schema_version: i64,
    pub targets: Vec<ProjectionStatusEntry>,
    pub webhook_lag_seconds: Option<i64>,
    pub rate_limit_remaining: Option<i64>,
}

/// A best-effort provider push that hasn't propagated to the forge yet (slice
/// γ). Surfaced read-only in admin so operators can see what's un-synced
/// instead of inferring it from the audit log.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingSyncEntry {
    pub id: i64,
    pub org: String,
    pub repo: String,
    pub branch: String,
    pub brain_id: String,
    pub kind: String,
    pub attempts: i64,
    pub last_attempt_at: String,
    pub last_error: Option<String>,
}

#[server(LoadAuditLog, "/api", endpoint = "load_audit_log")]
pub async fn load_audit_log(
    kind: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<AuditEntry>, ApiError> {
    use crate::server::session;
    let _ = session::require_target_admin_session().await.map_err(sfe)?;
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
pub async fn list_sessions() -> Result<Vec<SessionEntry>, ApiError> {
    use crate::server::session;
    let _ = session::require_target_admin_session().await.map_err(sfe)?;
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

#[server(ListPendingSync, "/api", endpoint = "list_pending_sync")]
pub async fn list_pending_sync() -> Result<Vec<PendingSyncEntry>, ApiError> {
    use crate::server::session;
    let _ = session::require_target_admin_session().await.map_err(sfe)?;
    let rows = crate::server::projection::list_all_pending_sync(200)
        .await
        .map_err(sfe)?;
    Ok(rows
        .into_iter()
        .map(|r| PendingSyncEntry {
            id: r.id,
            org: r.org,
            repo: r.repo,
            branch: r.branch,
            brain_id: r.brain_id,
            kind: r.kind,
            attempts: r.attempts,
            last_attempt_at: r.last_attempt_at,
            last_error: r.last_error,
        })
        .collect())
}

#[server(RevokeSession, "/api", endpoint = "revoke_session")]
pub async fn revoke_session(id: String) -> Result<u64, ApiError> {
    use crate::server::session;
    let s = session::require_target_admin_session().await.map_err(sfe)?;
    let actor = crate::server::auth::get_session_user(&s).await;
    let n = crate::server::audit::revoke_session(&id)
        .await
        .map_err(|e| sfe(BrainError::other(format!("DB: {e}"))))?;
    crate::server::audit::log("revoke_session", actor.as_deref(), &id).await;
    Ok(n)
}

#[server(GetProjectionStatus, "/api", endpoint = "get_projection_status")]
pub async fn get_projection_status() -> Result<ProjectionStatus, ApiError> {
    use crate::server::session;
    let _ = session::require_target_admin_session().await.map_err(sfe)?;
    let (schema_version, rows) = crate::server::projection::projection_status()
        .await
        .map_err(sfe)?;
    Ok(ProjectionStatus {
        schema_version,
        targets: rows
            .into_iter()
            .map(|r| ProjectionStatusEntry {
                org: r.org,
                repo: r.repo,
                branch: r.branch,
                status: r.status,
                last_attempt_at: r.last_attempt_at,
                last_success_at: r.last_success_at,
                last_error_at: r.last_error_at,
                last_error: r.last_error,
                last_reason: r.last_reason,
                file_count: r.file_count,
                node_count: r.node_count,
                edge_count: r.edge_count,
                work_item_count: r.work_item_count,
                last_rebuild_duration_ms: r.last_rebuild_duration_ms,
            })
            .collect(),
        webhook_lag_seconds: None,
        rate_limit_remaining: None,
    })
}

#[server(GetCurrentUser, "/api", endpoint = "get_current_user")]
pub async fn get_current_user() -> Result<Option<String>, ApiError> {
    use crate::server::session;
    let s = session::require_authenticated().await.map_err(sfe)?;
    Ok(crate::server::auth::get_session_user(&s).await)
}

#[server(LoadBrainTemplate, "/api", endpoint = "load_brain_template")]
pub async fn load_brain_template(target: TargetRef, node_type: String) -> Result<String, ApiError> {
    use crate::server::session;
    use gitnodes_storage::Storage;
    let target = super::target_from_ref(target).map_err(sfe)?;
    let (_s, token, _permissions) = session::require_target_read(&target).await.map_err(sfe)?;
    let config = crate::knowledge::config_loader::load(&target, &token).await;
    let Some(filename) = config
        .lookup(&node_type)
        .and_then(|s| s.template_filename.as_deref())
    else {
        return Ok(String::new());
    };
    if crate::server::local::is_enabled() {
        let raw = crate::server::local::read_template(filename)
            .map_err(gitnodes_domain::BrainError::Io)
            .map_err(sfe)?;
        let (body, _front) = crate::markdown::split_frontmatter(&raw);
        return Ok(body.trim_start_matches('\n').to_string());
    }
    let storage = session::storage_for(target).map_err(sfe)?;
    let raw = storage.load_template(&token, filename).await.map_err(sfe)?;
    let (body, _front) = crate::markdown::split_frontmatter(&raw);
    Ok(body.trim_start_matches('\n').to_string())
}
