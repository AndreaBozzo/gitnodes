use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use brain_domain::{BrainConfig, BrandConfig, TargetConfig, TargetRef, ViewSpec};

#[cfg(feature = "ssr")]
use super::WriteMode;
use super::WriteResult;
#[cfg(feature = "ssr")]
use super::sfe;
#[cfg(feature = "ssr")]
use super::write_orchestrator::{rebuild_projection_after_write, save_file_permission_aware};
#[cfg(feature = "ssr")]
use brain_domain::BrainError;

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
pub async fn list_views(target: TargetRef) -> Result<Vec<ViewSpec>, ServerFnError> {
    use crate::knowledge::config_loader;
    use crate::server::session;
    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let target = super::target_from_ref(target).map_err(sfe)?;
    let cfg = config_loader::load(&target, &token).await;
    Ok(cfg.views.clone())
}

/// Replace the entire `views` block in `.brain-config.yml` with the supplied
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
) -> Result<WriteResult, ServerFnError> {
    use crate::knowledge::config_loader;
    use crate::server::session;
    use brain_storage::Storage;

    let (s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let user = session::session_user_or_fallback(&s).await;
    let target = super::target_from_ref(target).map_err(sfe)?;
    let storage = session::storage_for(target.clone()).map_err(sfe)?;

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

#[server(GetCurrentUser, "/api", endpoint = "get_current_user")]
pub async fn get_current_user() -> Result<Option<String>, ServerFnError> {
    use crate::server::session;
    let s = session::session().map_err(sfe)?;
    Ok(crate::server::auth::get_session_user(&s).await)
}

#[server(LoadBrainTemplate, "/api", endpoint = "load_brain_template")]
pub async fn load_brain_template(
    target: TargetRef,
    node_type: String,
) -> Result<String, ServerFnError> {
    use crate::server::session;
    use brain_storage::Storage;
    let target = super::target_from_ref(target).map_err(sfe)?;
    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let config = crate::knowledge::config_loader::load(&target, &token).await;
    let Some(filename) = config
        .lookup(&node_type)
        .and_then(|s| s.template_filename.as_deref())
    else {
        return Ok(String::new());
    };
    let storage = session::storage_for(target).map_err(sfe)?;
    let raw = storage.load_template(&token, filename).await.map_err(sfe)?;
    let (body, _front) = crate::markdown::split_frontmatter(&raw);
    Ok(body.trim_start_matches('\n').to_string())
}
