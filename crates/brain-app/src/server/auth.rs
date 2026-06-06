//! Axum handlers for the OAuth flow. Business logic lives in `brain-auth`;
//! this module is the thin glue that emits audit events and HTTP redirects.

use axum::{
    extract::Query,
    response::{IntoResponse, Redirect},
};
use brain_auth::{
    SESSION_STATE_KEY, SESSION_TOKEN_KEY, SESSION_USER_KEY, authorize_url, exchange_code,
    fetch_user_login, generate_state, is_org_member,
};
use serde::Deserialize;
use tower_sessions::Session;

// Re-exported session helpers so existing callers keep working.
pub use brain_auth::{get_session_token, get_session_user, is_authenticated};

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

fn required_env_with_legacy(primary: &str, legacy: &str) -> String {
    std::env::var(primary)
        .or_else(|_| std::env::var(legacy))
        .unwrap_or_else(|_| {
            tracing::error!(
                "missing required environment variable: set {primary} (or legacy {legacy})"
            );
            std::process::exit(1)
        })
}

fn required_org() -> String {
    required_env_with_legacy("TARGET_GITHUB_ORG", "GITHUB_ORG")
}

fn resolve_login_org(explicit: Option<&str>, target_org: &str) -> Option<String> {
    explicit
        .map(str::trim)
        .map(str::to_string)
        .filter(|org| !org.is_empty())
        .or_else(|| {
            if explicit.is_none() {
                Some(target_org.to_string())
            } else {
                None
            }
        })
}

/// Optional organization allowlist for login.
///
/// An unset `GITHUB_LOGIN_ORG` preserves the historical behavior by falling
/// back to `TARGET_GITHUB_ORG`. Setting it to an empty value enables org-less
/// login; target access is still checked live against repository permissions.
pub fn login_org() -> Option<String> {
    match std::env::var("GITHUB_LOGIN_ORG") {
        Ok(value) => resolve_login_org(Some(&value), &required_org()),
        Err(std::env::VarError::NotPresent) => resolve_login_org(None, &required_org()),
        Err(std::env::VarError::NotUnicode(_)) => {
            tracing::error!("GITHUB_LOGIN_ORG contains non-Unicode data");
            std::process::exit(1)
        }
    }
}

/// Handler for `GET /auth/login`.
pub async fn login(session: Session) -> impl IntoResponse {
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
        assert_eq!(resolve_login_org(None, "acme"), Some("acme".to_string()));
    }

    #[test]
    fn explicit_login_org_overrides_target_owner() {
        assert_eq!(
            resolve_login_org(Some("contributors"), "octocat"),
            Some("contributors".to_string())
        );
    }

    #[test]
    fn empty_login_org_enables_org_less_mode() {
        assert_eq!(resolve_login_org(Some(""), "octocat"), None);
        assert_eq!(resolve_login_org(Some("  "), "octocat"), None);
    }
}
