//! GitHub OAuth primitives for gitnodes.
//!
//! Pure-ish building blocks — no axum, no audit side effects. Callers
//! (gitnodes's axum handlers) compose these with their own logging and
//! redirect behavior.

use gitnodes_domain::BrainError;
use serde::Deserialize;
use tower_sessions::Session;

pub const SESSION_TOKEN_KEY: &str = "github_token";
pub const SESSION_USER_KEY: &str = "github_user";
pub const SESSION_STATE_KEY: &str = "oauth_state";

/// Generate a random, URL-safe CSRF state string.
pub fn generate_state() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes)
}

/// Build the `github.com/login/oauth/authorize` URL with the given state.
pub fn authorize_url(client_id: &str, state: &str) -> String {
    format!(
        "https://github.com/login/oauth/authorize?client_id={client_id}&scope=repo+read:org&state={state}"
    )
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[derive(Deserialize)]
struct GitHubUser {
    login: String,
}

/// Exchange a one-shot `code` for an access token.
pub async fn exchange_code(
    client: &reqwest::Client,
    client_id: &str,
    client_secret: &str,
    code: &str,
) -> Result<String, BrainError> {
    let resp = client
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("code", code),
        ])
        .send()
        .await
        .map_err(|e| BrainError::github(format!("token exchange: {e}")))?;
    let body: TokenResponse = resp
        .json()
        .await
        .map_err(|e| BrainError::github(format!("token parse: {e}")))?;
    Ok(body.access_token)
}

/// Fetch the authenticated user's GitHub login.
pub async fn fetch_user_login(client: &reqwest::Client, token: &str) -> Result<String, BrainError> {
    let resp = client
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "gitnodes")
        .send()
        .await
        .map_err(|e| BrainError::github(format!("user fetch: {e}")))?;
    let body: GitHubUser = resp
        .json()
        .await
        .map_err(|e| BrainError::github(format!("user parse: {e}")))?;
    Ok(body.login)
}

/// Check `GET /orgs/{org}/members/{user}` — 204 means the user is a public or
/// visible-to-us member of `org`.
pub async fn is_org_member(client: &reqwest::Client, token: &str, org: &str, login: &str) -> bool {
    let url = format!("https://api.github.com/orgs/{org}/members/{login}");
    match client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "gitnodes")
        .send()
        .await
    {
        Ok(resp) => resp.status() == reqwest::StatusCode::NO_CONTENT,
        Err(_) => false,
    }
}

// --- session helpers ------------------------------------------------------

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

pub async fn is_authenticated(session: &Session) -> bool {
    matches!(session.get::<String>(SESSION_TOKEN_KEY).await, Ok(Some(_)))
}
