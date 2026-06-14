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

//! Session-extract helpers used by server fns.
//!
//! Collapses the `use_context::<Session>()?` + `get_session_token(..)?` dance
//! that every authed `#[server]` fn was repeating.

use gitnodes_domain::{BrainError, TargetConfig};
use gitnodes_storage::{GithubHttp, GithubStorage};
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
/// loading `.gitnodes.yml` for a specific repo) instead of the
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
    // Preview mode has no forge token; reads are served from the local
    // projection and writes are denied downstream.
    if super::local::is_enabled() {
        return Ok((s, String::new()));
    }
    let token = auth::get_session_token(&s)
        .await
        .ok_or(BrainError::Unauthenticated)?;
    Ok((s, token))
}

/// Require live read access on an explicit target before serving either forge
/// data or target-scoped data from the local projection.
pub async fn require_target_read(
    target: &TargetConfig,
) -> Result<(Session, String, gitnodes_storage::RepositoryPermissions), BrainError> {
    // Preview mode: read-only access to the local working tree, no forge call.
    if super::local::is_enabled() {
        return Ok((
            session()?,
            String::new(),
            super::local::read_only_permissions(),
        ));
    }
    let (session, token) = require_session_and_token().await?;
    let storage = storage_for(target.clone())?;
    let permissions = super::access::require_read(&storage, &token).await?;
    Ok((session, token, permissions))
}

/// Require live read access on the request-context target.
pub async fn require_current_target_read() -> Result<
    (
        Session,
        String,
        TargetConfig,
        gitnodes_storage::RepositoryPermissions,
    ),
    BrainError,
> {
    let target = target_cfg()?;
    let (session, token, permissions) = require_target_read(&target).await?;
    Ok((session, token, target, permissions))
}

/// Pull the GitHub login recorded in the session, or a fallback for commit
/// attribution when the session predates the user field.
pub async fn session_user_or_fallback(s: &Session) -> String {
    auth::get_session_user(s)
        .await
        .unwrap_or_else(|| "gitnodes".to_string())
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

/// Gate privileged admin surfaces on live admin or maintain access to the
/// current target repository.
///
/// Deployment-wide operator data still uses this target-admin gate for
/// backward compatibility. Splitting deployment administration from target
/// administration is tracked as the next open-source security slice.
pub async fn require_target_admin_session() -> Result<Session, BrainError> {
    // Preview mode is read-only and unauthenticated: admin surfaces are denied
    // outright so no forge call is attempted with an empty token.
    if super::local::is_enabled() {
        return Err(BrainError::Unauthenticated);
    }
    let (session, token) = require_session_and_token().await?;
    let target = target_cfg()?;
    let storage = storage_for(target)?;
    super::access::require_admin(&storage, &token).await?;

    Ok(session)
}
