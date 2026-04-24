//! Authenticated passthrough for repo assets.
//!
//! `raw.githubusercontent.com` requires the same OAuth token the app uses for
//! API calls when the backing repo is private, and the browser's `<img>` tag
//! can't carry that token. This handler fetches via the Contents API with the
//! session token, decodes, and serves bytes back with a mime type guessed from
//! the extension.
//!
//! Only paths under `assets/` are allowed so a compromised client can't read
//! arbitrary repo files through the proxy.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::Engine;
use brain_storage::{ContentResponse, http_client};
use tower_sessions::Session;

use super::auth;
use brain_domain::TargetConfig;

/// Axum state for the asset handler — just the target repo. We pull the token
/// off the session at request time.
#[derive(Clone)]
pub struct AssetProxyState {
    pub target: TargetConfig,
}

pub async fn serve_asset(
    State(state): State<AssetProxyState>,
    session: Session,
    Path(path): Path<String>,
) -> Response {
    if !auth::is_authenticated(&session).await {
        return (StatusCode::UNAUTHORIZED, "not authenticated").into_response();
    }
    let Some(token) = auth::get_session_token(&session).await else {
        return (StatusCode::UNAUTHORIZED, "no github token").into_response();
    };

    // `path` is the `*path` capture, so it's the part after `/assets/`. Rebuild
    // the repo-rooted path and refuse anything that escapes `assets/`.
    let repo_path = format!("assets/{}", path.trim_start_matches('/'));
    if repo_path.contains("..") {
        return (StatusCode::BAD_REQUEST, "invalid path").into_response();
    }

    let gh = brain_domain::GithubClient::new(state.target.clone());
    let url = format!(
        "{}?ref={}",
        gh.contents_url(&repo_path),
        state.target.branch
    );
    let Ok(client) = http_client() else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "http client").into_response();
    };
    let resp = match client.get(&url).bearer_auth(&token).send().await {
        Ok(r) => r,
        Err(_) => return (StatusCode::BAD_GATEWAY, "upstream fetch").into_response(),
    };
    if !resp.status().is_success() {
        let code = match resp.status().as_u16() {
            404 => StatusCode::NOT_FOUND,
            401 | 403 => StatusCode::FORBIDDEN,
            _ => StatusCode::BAD_GATEWAY,
        };
        return (code, "upstream non-success").into_response();
    }
    let body: ContentResponse = match resp.json().await {
        Ok(b) => b,
        Err(_) => return (StatusCode::BAD_GATEWAY, "upstream parse").into_response(),
    };
    let cleaned: String = body
        .content
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let bytes = match base64::engine::general_purpose::STANDARD.decode(cleaned) {
        Ok(b) => b,
        Err(_) => return (StatusCode::BAD_GATEWAY, "upstream decode").into_response(),
    };

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(mime_for(&repo_path)),
    );
    // Short cache — keeps the browser from re-fetching on every graph rebuild,
    // but lets a freshly-uploaded replacement propagate within a minute.
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=60"),
    );
    (StatusCode::OK, headers, bytes).into_response()
}

fn mime_for(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}
