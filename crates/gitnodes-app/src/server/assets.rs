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

//! Authenticated passthrough for repo assets.
//!
//! For private repos, `raw.githubusercontent.com` does not accept bearer auth
//! on the `Authorization` header — it returns 404 silently. The Contents API
//! (`/repos/{org}/{repo}/contents/{path}?ref={branch}`) does accept bearer
//! tokens and is the only stable way to fetch private blobs from a server-side
//! proxy. This handler uses the Contents API with `Accept: application/vnd.github.raw`
//! to receive the raw bytes directly (no base64 round-trip) and serves them
//! back to the browser with a mime type guessed from the extension.
//!
//! Only paths under `assets/` are allowed so a compromised client can't read
//! arbitrary repo files through the proxy.

use axum::{
    body::Body,
    extract::{OriginalUri, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use gitnodes_domain::{GithubClient, TargetConfig, decode_path_segment};
use gitnodes_storage::GithubHttp;
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
) -> Response {
    if !auth::is_authenticated(&session).await {
        return (StatusCode::UNAUTHORIZED, "not authenticated").into_response();
    }

    // Parse the asset sub-path from the full URI rather than via a `Path<String>`
    // extractor. The handler is mounted under TWO nests (`/assets` and
    // `/{org}/{repo}/assets`) — the multi-tenant nest puts `org` and `repo` in
    // the path-param map alongside `*path`, which makes a single-field
    // `Path<String>` deserialiser fail with a 500. Reading from `OriginalUri`
    // sidesteps the extractor entirely.
    let Some(asset_subpath) = subpath_after_assets(uri.path()) else {
        return (StatusCode::BAD_REQUEST, "invalid path").into_response();
    };
    let repo_path = format!(
        "assets/{}",
        decode_repo_path(asset_subpath.trim_start_matches('/'))
    );
    if repo_path.contains("..") {
        return (StatusCode::BAD_REQUEST, "invalid path").into_response();
    }

    // Preview mode serves assets straight from the working tree — there is no
    // forge to proxy. The token check below would otherwise reject the request
    // (preview has no session token).
    if crate::server::local::is_enabled() {
        if let Some(target) = target_from_path(uri.path(), &state.target)
            && crate::server::local::ensure_target(&target).is_err()
        {
            return (StatusCode::NOT_FOUND, "asset target not found").into_response();
        }
        return match crate::server::local::read_asset(&repo_path) {
            Ok(bytes) => serve_bytes(bytes, mime_for(&repo_path)),
            Err(error) => {
                tracing::debug!(%repo_path, %error, "preview asset not found");
                (StatusCode::NOT_FOUND, "asset not found").into_response()
            }
        };
    }

    let Some(token) = auth::get_session_token(&session).await else {
        return (StatusCode::UNAUTHORIZED, "no github token").into_response();
    };

    let target =
        target_from_path(uri.path(), &state.target).unwrap_or_else(|| state.target.clone());
    let gh = GithubClient::new(target.clone());
    // Use Contents API with `Accept: application/vnd.github.raw` rather than
    // raw.githubusercontent.com. For private repos, raw.* silently 404s on
    // bearer auth. This authenticated fetch is also the authorization gate:
    // GitHub returns 401/403/404 when the session token cannot read the repo,
    // avoiding a second permissions API request for every image on the page.
    let url = gh.contents_url(&repo_path);
    fetch_and_serve(
        &state.http,
        &url,
        &target.branch,
        &token,
        mime_for(&repo_path),
    )
    .await
}

/// Fetch raw bytes from a Contents API URL and turn them into the response we
/// hand back to the browser. Extracted from `serve_asset` so the GitHub-side
/// behavior can be exercised under wiremock without having to construct a
/// session, an `OriginalUri`, and an axum `State` extractor by hand.
async fn fetch_and_serve(
    http: &GithubHttp,
    url: &str,
    ref_name: &str,
    token: &str,
    content_type: &'static str,
) -> Response {
    let resp = match http
        .client()
        .get(url)
        .query(&[("ref", ref_name)])
        .bearer_auth(token)
        .header(header::ACCEPT, "application/vnd.github.raw")
        .header(header::USER_AGENT, "gitnodes")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(%url, error = %e, "asset proxy: upstream fetch failed");
            return (StatusCode::BAD_GATEWAY, "upstream fetch").into_response();
        }
    };
    let upstream_status = resp.status();
    if !upstream_status.is_success() {
        let body_snippet: String = resp
            .text()
            .await
            .unwrap_or_default()
            .chars()
            .take(256)
            .collect();
        tracing::warn!(
            %url,
            status = %upstream_status,
            body = %body_snippet,
            "asset proxy: upstream non-success",
        );
        let code = match upstream_status.as_u16() {
            404 => StatusCode::NOT_FOUND,
            401 | 403 => StatusCode::FORBIDDEN,
            _ => StatusCode::BAD_GATEWAY,
        };
        return (code, "upstream non-success").into_response();
    }
    let bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(%url, error = %e, "asset proxy: upstream body read failed");
            return (StatusCode::BAD_GATEWAY, "upstream read").into_response();
        }
    };

    tracing::debug!(%url, bytes = bytes.len(), content_type, "asset proxy: served");

    serve_bytes(bytes, content_type)
}

/// Wrap raw asset bytes in a response with an explicit Content-Type. Returning
/// `(StatusCode, HeaderMap, Vec<u8>)` works in many cases but tuple-element
/// ordering with `Vec<u8>::into_response()` (which sets its own
/// `application/octet-stream`) is fragile — being explicit avoids the browser
/// receiving image bytes labeled as octet-stream and refusing to render them.
fn serve_bytes(bytes: impl Into<Body>, content_type: &'static str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        // Short cache — keeps the browser from re-fetching on every graph
        // rebuild, but lets a freshly-uploaded replacement propagate within a
        // minute.
        .header(header::CACHE_CONTROL, "private, max-age=60")
        .body(bytes.into())
        .unwrap_or_else(|_| (StatusCode::INTERNAL_SERVER_ERROR, "build response").into_response())
}

/// Extract the part of the URL after `/assets/`. Works for both the legacy
/// `/assets/{path}` route and the multi-tenant `/{org}/{repo}/assets/{path}`
/// route by anchoring on the literal `/assets/` separator. Returns `None` if
/// the URL does not contain that segment (which would be a routing bug).
fn subpath_after_assets(uri_path: &str) -> Option<&str> {
    uri_path.split_once("/assets/").map(|(_, rest)| rest)
}

fn decode_repo_path(path: &str) -> String {
    path.split('/')
        .map(decode_path_segment)
        .collect::<Vec<_>>()
        .join("/")
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

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Minimal 1×1 transparent PNG. Lets us assert the proxy returns the exact
    /// bytes the upstream returned without dragging a fixture file along.
    const PNG_BYTES: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    #[tokio::test]
    async fn fetch_and_serve_returns_bytes_with_image_content_type() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(
                "/repos/Dritara-Digital/Brain/contents/assets/2026/04/foo.png",
            ))
            .and(query_param("ref", "main"))
            .and(header("authorization", "Bearer test-token"))
            .and(header("accept", "application/vnd.github.raw"))
            .and(header("user-agent", "gitnodes"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/vnd.github.raw; charset=utf-8")
                    .set_body_bytes(PNG_BYTES),
            )
            .expect(1)
            .mount(&server)
            .await;

        let http = GithubHttp::new().expect("http client");
        let url = format!(
            "{}/repos/Dritara-Digital/Brain/contents/assets/2026/04/foo.png",
            server.uri()
        );

        let resp = fetch_and_serve(&http, &url, "main", "test-token", "image/png").await;

        assert_eq!(resp.status(), StatusCode::OK);
        // Crucial: the browser refuses to render PNG bytes labeled as
        // `application/octet-stream`. Lock the proxy's content type to the
        // mime we picked from the extension.
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            "image/png",
        );
        assert_eq!(
            resp.headers().get(header::CACHE_CONTROL).unwrap(),
            "private, max-age=60",
        );
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(body.as_ref(), PNG_BYTES);
    }

    #[tokio::test]
    async fn fetch_and_serve_maps_404_to_not_found() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404).set_body_string("{\"message\":\"Not Found\"}"))
            .mount(&server)
            .await;
        let http = GithubHttp::new().expect("http client");
        let url = format!("{}/repos/x/y/contents/assets/missing.png", server.uri());
        let resp = fetch_and_serve(&http, &url, "main", "tok", "image/png").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn fetch_and_serve_maps_403_to_forbidden() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403).set_body_string("forbidden"))
            .mount(&server)
            .await;
        let http = GithubHttp::new().expect("http client");
        let url = format!("{}/repos/x/y/contents/assets/no.png", server.uri());
        let resp = fetch_and_serve(&http, &url, "main", "tok", "image/png").await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn target_from_path_extracts_org_repo() {
        let fallback = TargetConfig {
            org: "fallback-org".into(),
            repo: "fallback-repo".into(),
            branch: "main".into(),
        };
        let target =
            target_from_path("/Dritara-Digital/Brain/assets/2026/04/foo.png", &fallback).unwrap();
        assert_eq!(target.org, "Dritara-Digital");
        assert_eq!(target.repo, "Brain");
        // Branch always inherited from fallback — multi-tenant routes don't
        // carry the branch in the URL.
        assert_eq!(target.branch, "main");
    }

    #[test]
    fn target_from_path_returns_none_for_legacy_route() {
        let fallback = TargetConfig {
            org: "fallback-org".into(),
            repo: "fallback-repo".into(),
            branch: "main".into(),
        };
        // `/assets/...` (no org/repo) must fall through so the caller uses the
        // env-configured target.
        assert!(target_from_path("/assets/2026/04/foo.png", &fallback).is_none());
    }

    #[test]
    fn subpath_after_assets_handles_both_routes() {
        // Legacy single-target route.
        assert_eq!(
            subpath_after_assets("/assets/2026/04/foo.png"),
            Some("2026/04/foo.png")
        );
        // Multi-tenant route — `org` and `repo` segments are stripped.
        assert_eq!(
            subpath_after_assets("/Dritara-Digital/Brain/assets/2026/04/foo.png"),
            Some("2026/04/foo.png")
        );
        // No `/assets/` segment at all → None (routing bug).
        assert!(subpath_after_assets("/knowledge").is_none());
    }

    #[test]
    fn mime_for_handles_known_extensions() {
        assert_eq!(mime_for("a.png"), "image/png");
        assert_eq!(mime_for("a.PNG"), "image/png");
        assert_eq!(mime_for("a.jpg"), "image/jpeg");
        assert_eq!(mime_for("a.jpeg"), "image/jpeg");
        assert_eq!(mime_for("a.svg"), "image/svg+xml");
        assert_eq!(mime_for("a.unknown"), "application/octet-stream");
    }
}
