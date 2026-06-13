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

//! Axum handlers for the OAuth flow. Business logic lives in `gitnodes-auth`;
//! this module is the thin glue that emits audit events and HTTP redirects.

use axum::{
    extract::Query,
    response::{IntoResponse, Redirect},
};
use gitnodes_auth::{
    SESSION_STATE_KEY, SESSION_TOKEN_KEY, SESSION_USER_KEY, authorize_url, exchange_code,
    fetch_user_login, generate_state, is_org_member,
};
use serde::Deserialize;
use std::sync::OnceLock;
use tower_sessions::Session;

/// PAT-aware session token. In single-user PAT mode every request authenticates
/// with the operator's PAT; otherwise the token comes from the OAuth session.
pub async fn get_session_token(session: &Session) -> Option<String> {
    if let Some(pat) = crate::server::pat::identity() {
        return Some(pat.token.clone());
    }
    gitnodes_auth::get_session_token(session).await
}

/// PAT-aware login (the PAT owner in single-user mode).
pub async fn get_session_user(session: &Session) -> Option<String> {
    if let Some(pat) = crate::server::pat::identity() {
        return Some(pat.login.clone());
    }
    gitnodes_auth::get_session_user(session).await
}

/// PAT-aware authentication check. In PAT mode the single operator is always
/// authenticated; otherwise a live OAuth session is required.
pub async fn is_authenticated(session: &Session) -> bool {
    crate::server::pat::is_enabled() || gitnodes_auth::is_authenticated(session).await
}

static LOGIN_ORG: OnceLock<Option<String>> = OnceLock::new();

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        tracing::error!("missing required environment variable: {name}");
        std::process::exit(1)
    })
}

fn client_id() -> String {
    required_env("GITHUB_CLIENT_ID")
}

fn client_secret() -> String {
    required_env("GITHUB_CLIENT_SECRET")
}

fn resolve_login_org(
    explicit: Option<&str>,
    target_org: &str,
    compact_locator: bool,
) -> Option<String> {
    explicit
        .map(str::trim)
        .map(str::to_string)
        .filter(|org| !org.is_empty())
        .or_else(|| {
            if explicit.is_none() && !compact_locator {
                Some(target_org.to_string())
            } else {
                None
            }
        })
}

/// Optional organization allowlist for login.
///
/// Load the login organization policy once during server startup.
pub fn init_login_org(target_org: &str, compact_locator: bool) -> Result<(), String> {
    let explicit = match std::env::var("GITHUB_LOGIN_ORG") {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err("GITHUB_LOGIN_ORG contains non-Unicode data".into());
        }
    };
    let login_org = resolve_login_org(explicit.as_deref(), target_org, compact_locator);
    LOGIN_ORG
        .set(login_org)
        .map_err(|_| "login organization policy was initialized more than once".into())
}

pub fn login_org() -> Option<String> {
    LOGIN_ORG
        .get()
        .expect("login organization policy initialized at startup")
        .clone()
}

/// Handler for `GET /auth/login`.
pub async fn login(session: Session) -> impl IntoResponse {
    // PAT mode has no OAuth App; the single operator is always signed in.
    if crate::server::pat::is_enabled() {
        return Redirect::to("/knowledge").into_response();
    }
    let state = generate_state();
    if session.insert(SESSION_STATE_KEY, &state).await.is_err() {
        return Redirect::to("/?error=session_init").into_response();
    }
    if session.save().await.is_err() {
        return Redirect::to("/?error=session_save").into_response();
    }
    Redirect::to(&authorize_url(&client_id(), &state)).into_response()
}

/// Handler for `GET /auth/logout`.
pub async fn logout(session: Session) -> impl IntoResponse {
    let user = get_session_user(&session).await;
    let _ = session.flush().await;
    crate::server::audit::log("logout", user.as_deref(), "").await;
    Redirect::to("/").into_response()
}

#[derive(Deserialize)]
pub struct CallbackParams {
    code: String,
    state: Option<String>,
}

/// Handler for `GET /auth/callback?code=...&state=...`.
pub async fn oauth_callback(
    Query(params): Query<CallbackParams>,
    session: Session,
) -> impl IntoResponse {
    let expected_state = session
        .remove::<String>(SESSION_STATE_KEY)
        .await
        .ok()
        .flatten();

    // Distinguish "no state cookie at all" (cookie dropped between /auth/login
    // and the GitHub redirect — usually a SameSite/Secure or session-store
    // problem) from "cookie present but value differs" (replay, double-submit,
    // or stolen link). Same redirect to the user; different audit reason.
    match (&expected_state, &params.state) {
        (Some(expected), Some(got)) if expected == got => {}
        (None, _) => {
            crate::server::audit::log("login_fail", None, "state_missing").await;
            return Redirect::to("/?error=state_mismatch").into_response();
        }
        _ => {
            crate::server::audit::log("login_fail", None, "state_mismatch").await;
            return Redirect::to("/?error=state_mismatch").into_response();
        }
    }

    let client = reqwest::Client::new();

    let token = match exchange_code(&client, &client_id(), &client_secret(), &params.code).await {
        Ok(t) => t,
        Err(_) => {
            crate::server::audit::log("login_fail", None, "token_exchange").await;
            return Redirect::to("/?error=token_exchange").into_response();
        }
    };

    let login = match fetch_user_login(&client, &token).await {
        Ok(u) => u,
        Err(_) => {
            crate::server::audit::log("login_fail", None, "user_fetch").await;
            return Redirect::to("/?error=user_fetch").into_response();
        }
    };

    if let Some(org) = login_org()
        && !is_org_member(&client, &token, &org, &login).await
    {
        crate::server::audit::log("login_fail", Some(&login), "not_login_org_member").await;
        return Redirect::to("/?error=not_login_org_member").into_response();
    }

    // Cycle the session ID to prevent session fixation and to guarantee
    // the Set-Cookie header is present on the redirect response.
    if session.cycle_id().await.is_err() {
        return Redirect::to("/?error=session_write").into_response();
    }
    if session.insert(SESSION_TOKEN_KEY, &token).await.is_err() {
        return Redirect::to("/?error=session_write").into_response();
    }
    if session.insert(SESSION_USER_KEY, &login).await.is_err() {
        return Redirect::to("/?error=session_write").into_response();
    }
    if session.save().await.is_err() {
        return Redirect::to("/?error=session_save").into_response();
    }

    crate::server::audit::log("login_ok", Some(&login), "").await;
    Redirect::to("/knowledge").into_response()
}

#[cfg(test)]
mod tests {
    use super::resolve_login_org;

    #[test]
    fn unset_login_org_preserves_target_org_gate() {
        assert_eq!(
            resolve_login_org(None, "acme", false),
            Some("acme".to_string())
        );
    }

    #[test]
    fn explicit_login_org_overrides_target_owner() {
        assert_eq!(
            resolve_login_org(Some("contributors"), "octocat", false),
            Some("contributors".to_string())
        );
    }

    #[test]
    fn empty_login_org_enables_org_less_mode() {
        assert_eq!(resolve_login_org(Some(""), "octocat", false), None);
        assert_eq!(resolve_login_org(Some("  "), "octocat", false), None);
    }

    #[test]
    fn compact_locator_defaults_to_org_less_mode() {
        assert_eq!(resolve_login_org(None, "octocat", true), None);
    }
}
