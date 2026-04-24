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
/// Falls back to constructing a fresh per-call client only when no shared
/// instance was provided (test paths) so callers stay a single line.
pub fn github_http() -> Result<GithubHttp, BrainError> {
    if let Some(http) = use_context::<GithubHttp>() {
        return Ok(http);
    }
    let target = target_cfg()?;
    GithubHttp::new(target)
}

/// Build a `GithubStorage` from the shared pooled HTTP client. All server fns
/// that need storage should go through this helper instead of `GithubStorage::new`.
pub fn storage() -> Result<GithubStorage, BrainError> {
    Ok(GithubStorage::with_http(github_http()?))
}

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
