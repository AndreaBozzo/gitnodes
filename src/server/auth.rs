use axum::{
    body::Body,
    extract::{Query, Request},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use tower_sessions::Session;

const SESSION_TOKEN_KEY: &str = "github_token";
const SESSION_USER_KEY: &str = "github_user";
const SESSION_STATE_KEY: &str = "oauth_state";

/// The GitHub org that owns the Brain repo.
const REQUIRED_ORG: &str = "Dritara-Digital";

fn client_id() -> String {
    std::env::var("GITHUB_CLIENT_ID").expect("GITHUB_CLIENT_ID must be set")
}

fn client_secret() -> String {
    std::env::var("GITHUB_CLIENT_SECRET").expect("GITHUB_CLIENT_SECRET must be set")
}

/// Generate a random, URL-safe CSRF state string.
fn generate_state() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes)
}

/// Handler for `GET /auth/login`.
/// Generates a CSRF state, stores it in the session, and redirects to GitHub.
/// Requests `repo` + `read:org` scopes so we can verify org membership.
pub async fn login(session: Session) -> impl IntoResponse {
    let state = generate_state();
    if session.insert(SESSION_STATE_KEY, &state).await.is_err() {
        return Redirect::to("/?error=session_init").into_response();
    }
    if session.save().await.is_err() {
        return Redirect::to("/?error=session_save").into_response();
    }
    let client_id = client_id();
    let url = format!(
        "https://github.com/login/oauth/authorize?client_id={client_id}&scope=repo+read:org&state={state}"
    );
    Redirect::to(&url).into_response()
}

/// Handler for `GET /auth/logout`.
pub async fn logout(session: Session) -> impl IntoResponse {
    let _ = session.flush().await;
    Redirect::to("/").into_response()
}

#[derive(Deserialize)]
pub struct CallbackParams {
    code: String,
    state: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[derive(Deserialize)]
struct GitHubUser {
    login: String,
}

/// Handler for `GET /auth/callback?code=...&state=...`.
/// Verifies CSRF state, exchanges code for token, checks org membership,
/// stores token + user in session, then redirects to `/knowledge`.
pub async fn oauth_callback(
    Query(params): Query<CallbackParams>,
    session: Session,
) -> impl IntoResponse {
    let expected_state = session
        .remove::<String>(SESSION_STATE_KEY)
        .await
        .ok()
        .flatten();

    match (&expected_state, &params.state) {
        (Some(expected), Some(got)) if expected == got => {}
        _ => return Redirect::to("/?error=state_mismatch").into_response(),
    }

    let client = reqwest::Client::new();

    // --- Exchange code for access token ---
    let token_res = client
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .form(&[
            ("client_id", client_id()),
            ("client_secret", client_secret()),
            ("code", params.code),
        ])
        .send()
        .await;

    let token = match token_res {
        Ok(resp) => match resp.json::<TokenResponse>().await {
            Ok(t) => t.access_token,
            Err(_) => return Redirect::to("/?error=token_parse").into_response(),
        },
        Err(_) => return Redirect::to("/?error=token_exchange").into_response(),
    };

    // --- Fetch GitHub username ---
    let user_res = client
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "brain_ui")
        .send()
        .await;

    let login = match user_res {
        Ok(resp) => match resp.json::<GitHubUser>().await {
            Ok(u) => u.login,
            Err(_) => return Redirect::to("/?error=user_fetch").into_response(),
        },
        Err(_) => return Redirect::to("/?error=user_fetch").into_response(),
    };

    // --- Verify the user is a member of the Dritara-Digital org ---
    // GET /orgs/{org}/members/{username} → 204 = member, 404/302 = not
    let membership_url = format!("https://api.github.com/orgs/{REQUIRED_ORG}/members/{login}");
    let membership_ok = match client
        .get(&membership_url)
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "brain_ui")
        .send()
        .await
    {
        Ok(resp) => resp.status() == reqwest::StatusCode::NO_CONTENT,
        Err(_) => false,
    };

    if !membership_ok {
        return Redirect::to("/?error=not_org_member").into_response();
    }

    // --- Persist session (await errors instead of ignoring) ---
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

    Redirect::to("/knowledge").into_response()
}

/// Axum middleware that redirects unauthenticated requests to `/`.
/// Expects the `tower-sessions` layer to run before it.
pub async fn require_auth(session: Session, request: Request<Body>, next: Next) -> Response {
    match session.get::<String>(SESSION_TOKEN_KEY).await {
        Ok(Some(_)) => next.run(request).await,
        _ => Redirect::to("/").into_response(),
    }
}

pub async fn get_session_token(session: &Session) -> Option<String> {
    session
        .get::<String>(SESSION_TOKEN_KEY)
        .await
        .ok()
        .flatten()
}

pub async fn get_session_user(session: &Session) -> Option<String> {
    session.get::<String>(SESSION_USER_KEY).await.ok().flatten()
}

/// Server-side helper: is the current session authenticated?
pub async fn is_authenticated(session: &Session) -> bool {
    matches!(session.get::<String>(SESSION_TOKEN_KEY).await, Ok(Some(_)))
}
