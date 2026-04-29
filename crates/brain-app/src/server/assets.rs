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
    extract::{OriginalUri, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use brain_domain::{GithubClient, TargetConfig};
use brain_storage::GithubHttp;
use tower_sessions::Session;

use super::auth;

/// Axum state for the asset handler — the shared pooled HTTP transport plus
/// the explicit target the proxy serves from. Keeping these two fields
/// distinct (rather than baking the target into a `GithubHttp`) preserves the
/// transport's target-agnosticism and makes the active repo obvious in code.
#[derive(Clone)]
pub struct AssetProxyState {
    pub http: GithubHttp,
    pub target: TargetConfig,
}

pub async fn serve_asset(
    State(state): State<AssetProxyState>,
    session: Session,
    OriginalUri(uri): OriginalUri,
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

    let target =
        target_from_path(uri.path(), &state.target).unwrap_or_else(|| state.target.clone());
    let gh = GithubClient::new(target.clone());
    let url = format!("{}/{}", gh.raw_base(), repo_path);
    let resp = match state
        .http
        .client()
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
    {
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
    let bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(_) => return (StatusCode::BAD_GATEWAY, "upstream read").into_response(),
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
    (StatusCode::OK, headers, bytes.to_vec()).into_response()
}

fn target_from_path(path: &str, fallback: &TargetConfig) -> Option<TargetConfig> {
    let segments: Vec<&str> = path.trim_start_matches('/').splitn(4, '/').collect();
    match segments.as_slice() {
        [org, repo, "assets"] if !org.is_empty() && !repo.is_empty() => Some(TargetConfig {
            org: (*org).to_string(),
            repo: (*repo).to_string(),
            branch: fallback.branch.clone(),
        }),
        [org, repo, "assets", _] if !org.is_empty() && !repo.is_empty() => Some(TargetConfig {
            org: (*org).to_string(),
            repo: (*repo).to_string(),
            branch: fallback.branch.clone(),
        }),
        _ => None,
    }
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
