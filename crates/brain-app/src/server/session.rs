//! Session-extract helpers used by server fns.
//!
//! Collapses the `use_context::<Session>()?` + `get_session_token(..)?` dance
//! that every authed `#[server]` fn was repeating.

use brain_domain::{BrainError, TargetConfig};
use brain_storage::{GithubHttp, GithubStorage};
use leptos::prelude::use_context;
use tower_sessions::Session;

use super::auth;

/// Pull the Session out of the Leptos server context.
pub fn session() -> Result<Session, BrainError> {
    use_context::<Session>().ok_or_else(|| BrainError::other("No session available"))
}

/// Pull the `TargetConfig` out of the Leptos server context.
pub fn target_cfg() -> Result<TargetConfig, BrainError> {
    use_context::<TargetConfig>().ok_or_else(|| BrainError::other("No target config available"))
}

/// Pull the shared pooled `GithubHttp` out of the Leptos server context.
/// Falls back to constructing a fresh per-call transport only when no shared
/// instance was provided (test paths). The transport is target-agnostic, so
/// no fallback target is needed.
pub fn github_http() -> Result<GithubHttp, BrainError> {
    if let Some(http) = use_context::<GithubHttp>() {
        return Ok(http);
    }
    GithubHttp::new()
}

/// Build a `GithubStorage` for the **session's** target using the shared
/// pooled HTTP client. The target is read fresh from context every call so a
/// future Brain-Switcher (Phase 3) that swaps the per-request `TargetConfig`
/// is honoured immediately, without touching the pooled transport.
pub fn storage() -> Result<GithubStorage, BrainError> {
    let target = target_cfg()?;
    Ok(GithubStorage::new(github_http()?, target))
}

/// Build a `GithubStorage` for an **explicit** target. Use this when the
/// caller already has the right `TargetConfig` in hand (e.g. config_loader
/// loading `.brain-config.yml` for a specific repo) instead of the
/// session-default target. Reuses the shared pooled transport.
pub fn storage_for(target: TargetConfig) -> Result<GithubStorage, BrainError> {
    Ok(GithubStorage::new(github_http()?, target))
}

/// Source-audit marker for server fns that delegate their auth gate to an
/// inner helper. The `api.rs` regression test requires each non-public
/// `#[server]` fn body to contain a direct gate call or this marker.
pub(crate) fn __assert_gated() {}

/// Pull Session + GitHub token (fails with `Unauthenticated` if missing).
pub async fn require_session_and_token() -> Result<(Session, String), BrainError> {
    let s = session()?;
    let token = auth::get_session_token(&s)
        .await
        .ok_or(BrainError::Unauthenticated)?;
    Ok((s, token))
}

/// Pull the GitHub login recorded in the session, or a fallback for commit
/// attribution when the session predates the user field.
pub async fn session_user_or_fallback(s: &Session) -> String {
    auth::get_session_user(s)
        .await
        .unwrap_or_else(|| "brain_ui".to_string())
}

/// Gate admin-only server fns on a live session.
pub async fn require_authenticated() -> Result<Session, BrainError> {
    let s = session()?;
    if auth::is_authenticated(&s).await {
        Ok(s)
    } else {
        Err(BrainError::Unauthenticated)
    }
}

/// Gate privileged admin surfaces on a live session that still belongs to the
/// current target org and has admin or maintain access on the target repo.
pub async fn require_target_admin_session() -> Result<Session, BrainError> {
    let (session, token) = require_session_and_token().await?;
    let target = target_cfg()?;
    let storage = storage_for(target.clone())?;
    let permissions = storage.repository_permissions(&token).await?;
    if !(permissions.admin || permissions.maintain) {
        return Err(BrainError::other(
            "admin or maintain access on the target repository is required",
        ));
    }

    let client = reqwest::Client::new();
    let login = match auth::get_session_user(&session).await {
        Some(login) => login,
        None => brain_auth::fetch_user_login(&client, &token).await?,
    };

    if !brain_auth::is_org_member(&client, &token, &target.org, &login).await {
        let _ = session.flush().await;
        return Err(BrainError::Unauthenticated);
    }

    Ok(session)
}
