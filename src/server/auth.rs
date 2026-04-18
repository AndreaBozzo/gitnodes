use axum::{
    extract::Query,
    response::{IntoResponse, Redirect},
};
use serde::Deserialize;
use tower_sessions::Session;

const SESSION_TOKEN_KEY: &str = "github_token";
const SESSION_USER_KEY: &str = "github_user";

/// Environment-driven config. Set these in `.env` or your hosting env.
fn client_id() -> String {
    std::env::var("GITHUB_CLIENT_ID").expect("GITHUB_CLIENT_ID must be set")
}

fn client_secret() -> String {
    std::env::var("GITHUB_CLIENT_SECRET").expect("GITHUB_CLIENT_SECRET must be set")
}

/// Returns the GitHub OAuth authorize URL for the "Login with GitHub" button.
pub fn authorize_url() -> String {
    let client_id = client_id();
    format!("https://github.com/login/oauth/authorize?client_id={client_id}&scope=repo")
}

#[derive(Deserialize)]
pub struct CallbackParams {
    code: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[derive(Deserialize)]
struct GitHubUser {
    login: String,
}

/// Axum handler for `/auth/callback?code=...`
/// Exchanges the OAuth code for an access token, stores it in the session.
pub async fn oauth_callback(
    Query(params): Query<CallbackParams>,
    session: Session,
) -> impl IntoResponse {
    let client = reqwest::Client::new();

    // Exchange code for token
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

    // Fetch the user's login to store in session
    let user_res = client
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "brain_ui")
        .send()
        .await;

    let login = match user_res {
        Ok(resp) => match resp.json::<GitHubUser>().await {
            Ok(u) => u.login,
            Err(_) => "unknown".to_string(),
        },
        Err(_) => "unknown".to_string(),
    };

    let _ = session.insert(SESSION_TOKEN_KEY, &token).await;
    let _ = session.insert(SESSION_USER_KEY, &login).await;

    Redirect::to("/").into_response()
}

/// Retrieve the GitHub token from the current session (server-side only).
pub async fn get_session_token(session: &Session) -> Option<String> {
    session
        .get::<String>(SESSION_TOKEN_KEY)
        .await
        .ok()
        .flatten()
}

/// Retrieve the GitHub username from the current session.
pub async fn get_session_user(session: &Session) -> Option<String> {
    session.get::<String>(SESSION_USER_KEY).await.ok().flatten()
}
