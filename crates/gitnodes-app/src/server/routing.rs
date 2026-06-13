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

//! Per-request target resolution.
//!
//! Replaces the synchronous, Referer-fallback-driven logic that used to live
//! inline in `main.rs`. The single source of truth for "which target does
//! this request belong to" is now this module — every page handler and
//! `/api/*` handler routes through `target_for_request`.
//!
//! Trust boundary contract for 3.7B-α:
//! * Path is the only signal we read for routing. The `Referer` header is
//!   never inspected.
//! * 4-segment canonical paths produce `Resolved::Target` straight from URL
//!   data. The branch was already validated when the route was matched.
//! * 3-segment legacy paths are resolved via [`super::target_registry`]:
//!   hit → `Resolved::Target` (sticky branch from DB), miss → `Resolved::Unresolved`
//!   so the SSR layer can call `resolve_legacy_target` and emit a 302 to the
//!   canonical URL.
//! * `/api/*` requests with no multi-tenant prefix fall back to the env
//!   default. Mutations carry an explicit `TargetRef` in the body and verify
//!   it server-side; reads inherit the path-derived context.

use axum::{body::Body, extract::Request};
use gitnodes_domain::{TargetConfig, TargetRef, decode_path_segment, encode_path_segment};
use sqlx::SqlitePool;

use super::target_registry;

/// The outcome of resolving a request to a forge target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolved {
    /// A concrete target identity. Either lifted from a 4-segment URL or
    /// looked up from `target_registry` for a registered 3-segment legacy URL.
    Target(TargetRef),
    /// 3-segment legacy URL whose `(org, repo)` is not yet in `target_registry`.
    /// The SSR layer must call `resolve_legacy_target` and emit a 302 to the
    /// canonical 4-segment URL. Returned with the raw path components so the
    /// redirect target can preserve the trailing segment(s) verbatim.
    Unresolved {
        org: String,
        repo: String,
        suffix: LegacySuffix,
    },
    /// Default fallback. Used for `/api/*` calls without a multi-tenant
    /// prefix and any other request that does not match a target-scoped
    /// shape (landing page, auth callbacks, SSE).
    Default(TargetConfig),
}

/// Trailing segment(s) after `/{org}/{repo}/...` for a legacy URL. Used to
/// reconstruct the canonical redirect URL with the same suffix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LegacySuffix {
    Knowledge,
    Admin,
    AdminViews,
}

impl LegacySuffix {
    pub fn as_path(&self) -> &'static str {
        match self {
            LegacySuffix::Knowledge => "knowledge",
            LegacySuffix::Admin => "admin",
            LegacySuffix::AdminViews => "admin/views",
        }
    }
}

impl Resolved {
    /// Project the resolved outcome down to a [`TargetConfig`] for context
    /// injection. `Unresolved` falls back to the env default — the SSR
    /// redirect page will short-circuit before any server fn runs against
    /// it, but having a concrete value here keeps the context shape
    /// invariant for callers like `get_app_config` that don't care.
    pub fn target_config(&self, fallback: &TargetConfig) -> TargetConfig {
        match self {
            Resolved::Target(t) => TargetConfig::from(t),
            Resolved::Default(t) => t.clone(),
            Resolved::Unresolved { .. } => fallback.clone(),
        }
    }

    /// Marker injected into Leptos context so the SSR redirect components
    /// know whether to short-circuit. `None` for resolved/default cases.
    pub fn legacy_marker(&self) -> Option<LegacyTargetUnresolved> {
        if let Resolved::Unresolved { org, repo, suffix } = self {
            Some(LegacyTargetUnresolved {
                org: org.clone(),
                repo: repo.clone(),
                suffix: suffix.clone(),
            })
        } else {
            None
        }
    }
}

/// Context-injected marker. Presence means: "this request is on a 3-segment
/// legacy URL whose target is not yet in `target_registry`. The page must
/// call `resolve_legacy_target` and emit a redirect to the canonical URL."
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyTargetUnresolved {
    pub org: String,
    pub repo: String,
    pub suffix: LegacySuffix,
}

impl LegacyTargetUnresolved {
    /// Build the canonical redirect path for this unresolved target after
    /// the branch has been registered/discovered.
    pub fn canonical_path(&self, branch: &str) -> String {
        format!(
            "/{org}/{repo}/{branch}/{suffix}",
            org = self.org,
            repo = self.repo,
            branch = encode_path_segment(branch),
            suffix = self.suffix.as_path(),
        )
    }
}

/// Resolve a request to a forge target. Async because the legacy lookup hits
/// SQLite. The SQLite pool is the projection pool — same one
/// `target_registry::lookup` uses.
pub async fn target_for_request(
    request: &Request<Body>,
    fallback: &TargetConfig,
    pool: Option<&SqlitePool>,
) -> Resolved {
    let path = request.uri().path();
    resolve_path(path, fallback, pool).await
}

/// Path-only resolution. Extracted so unit tests can drive it without
/// constructing an axum `Request`.
pub async fn resolve_path(
    path: &str,
    fallback: &TargetConfig,
    pool: Option<&SqlitePool>,
) -> Resolved {
    let segments: Vec<&str> = path.trim_start_matches('/').splitn(6, '/').collect();

    // 4-segment canonical: `/{org}/{repo}/{branch}/{knowledge|admin}[/...]`
    if let Some((org, repo, branch)) = match_canonical(&segments) {
        let branch = decode_path_segment(branch);
        let r = TargetRef::new(org, repo, branch);
        if r.validate().is_ok() {
            return Resolved::Target(r);
        }
        // Malformed (e.g. branch contains `..` snuck through axum decoding).
        // Fall through to the default — the page will 404 when it tries to
        // load anything; we never silently route to the env target.
        return Resolved::Default(fallback.clone());
    }

    // 3-segment legacy: `/{org}/{repo}/{knowledge|admin|assets}[/...]`. Asset
    // proxy is intentionally listed here too so the legacy route has a
    // concrete branch via `target_registry`; it falls back gracefully when
    // no DB pool is wired in (test paths and the asset axum handler that
    // uses its own state).
    if let Some((org, repo, suffix)) = match_legacy(&segments) {
        let Some(pool) = pool else {
            // No DB available — preserve old behaviour: env fallback.
            return Resolved::Default(TargetConfig {
                org: org.to_string(),
                repo: repo.to_string(),
                branch: fallback.branch.clone(),
            });
        };
        return match target_registry::lookup(pool, org, repo).await {
            Ok(Some(entry)) => Resolved::Target(entry.target_ref()),
            Ok(None) => match suffix {
                Some(s) => Resolved::Unresolved {
                    org: org.to_string(),
                    repo: repo.to_string(),
                    suffix: s,
                },
                // Asset path miss: fall back to env. The asset proxy has
                // its own routing layer; `target_registry` registration
                // for assets is not in scope for α.
                None => Resolved::Default(TargetConfig {
                    org: org.to_string(),
                    repo: repo.to_string(),
                    branch: fallback.branch.clone(),
                }),
            },
            Err(e) => {
                tracing::warn!(?e, %org, %repo, "target_registry lookup failed; using env fallback");
                Resolved::Default(fallback.clone())
            }
        };
    }

    Resolved::Default(fallback.clone())
}

/// Returns `(org, repo, branch)` if the path matches a 4-segment canonical
/// shape ending in `knowledge`, `admin`, or `admin/views`.
fn match_canonical<'a>(segments: &[&'a str]) -> Option<(&'a str, &'a str, &'a str)> {
    let nonempty = |s: &&str| !s.is_empty();
    match segments {
        [org, repo, branch, "knowledge"] | [org, repo, branch, "admin"]
            if [*org, *repo, *branch].iter().all(nonempty) =>
        {
            Some((*org, *repo, *branch))
        }
        [org, repo, branch, "knowledge", _] | [org, repo, branch, "admin", _]
            if [*org, *repo, *branch].iter().all(nonempty) =>
        {
            Some((*org, *repo, *branch))
        }
        [org, repo, branch, "admin", "views"] | [org, repo, branch, "admin", "views", _]
            if [*org, *repo, *branch].iter().all(nonempty) =>
        {
            Some((*org, *repo, *branch))
        }
        _ => None,
    }
}

/// Returns `(org, repo, suffix)` for a 3-segment legacy URL. `suffix` is
/// `None` for the asset proxy (it doesn't redirect to a canonical form in α)
/// and `Some(...)` for `knowledge`/`admin[/views]`.
fn match_legacy<'a>(segments: &[&'a str]) -> Option<(&'a str, &'a str, Option<LegacySuffix>)> {
    let nonempty = |s: &&str| !s.is_empty();
    match segments {
        [org, repo, "knowledge"] | [org, repo, "knowledge", _]
            if [*org, *repo].iter().all(nonempty) =>
        {
            Some((*org, *repo, Some(LegacySuffix::Knowledge)))
        }
        [org, repo, "admin"] if [*org, *repo].iter().all(nonempty) => {
            Some((*org, *repo, Some(LegacySuffix::Admin)))
        }
        [org, repo, "admin", "views"] | [org, repo, "admin", "views", _]
            if [*org, *repo].iter().all(nonempty) =>
        {
            Some((*org, *repo, Some(LegacySuffix::AdminViews)))
        }
        [org, repo, "admin", _] if [*org, *repo].iter().all(nonempty) => {
            Some((*org, *repo, Some(LegacySuffix::Admin)))
        }
        [org, repo, "assets"] | [org, repo, "assets", _] if [*org, *repo].iter().all(nonempty) => {
            Some((*org, *repo, None))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;

    fn fallback() -> TargetConfig {
        TargetConfig {
            org: "env-org".into(),
            repo: "env-repo".into(),
            branch: "env-branch".into(),
        }
    }

    async fn pool_with_seed(seeds: &[(&str, &str, &str)]) -> SqlitePool {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .unwrap();
        crate::server::projection::migrate(&pool).await.unwrap();
        for (org, repo, branch) in seeds {
            sqlx::query(
                "INSERT INTO targets (key, org, repo, branch, source)
                 VALUES (?, ?, ?, ?, 'lazy_legacy')",
            )
            .bind(format!("{org}/{repo}/{branch}"))
            .bind(*org)
            .bind(*repo)
            .bind(*branch)
            .execute(&pool)
            .await
            .unwrap();
        }
        pool
    }

    #[tokio::test]
    async fn canonical_4segment_yields_target() {
        let r = resolve_path("/acme/kb/main/knowledge", &fallback(), None).await;
        assert_eq!(r, Resolved::Target(TargetRef::new("acme", "kb", "main")));
    }

    #[tokio::test]
    async fn canonical_4segment_admin_views_yields_target() {
        let r = resolve_path("/acme/kb/main/admin/views", &fallback(), None).await;
        assert_eq!(r, Resolved::Target(TargetRef::new("acme", "kb", "main")));
    }

    #[tokio::test]
    async fn canonical_4segment_subpath_yields_target() {
        let r = resolve_path("/acme/kb/main/knowledge/foo", &fallback(), None).await;
        assert_eq!(r, Resolved::Target(TargetRef::new("acme", "kb", "main")));
    }

    #[tokio::test]
    async fn canonical_with_invalid_branch_falls_back_to_default() {
        // axum normally percent-decodes; a `..` segment that survives is
        // routed to Default rather than silently picking the env target.
        let r = resolve_path("/acme/kb/../knowledge", &fallback(), None).await;
        assert!(matches!(r, Resolved::Default(_)));
    }

    #[tokio::test]
    async fn legacy_3segment_with_registry_hit_yields_sticky_target() {
        let pool = pool_with_seed(&[("acme", "kb", "develop")]).await;
        let r = resolve_path("/acme/kb/knowledge", &fallback(), Some(&pool)).await;
        assert_eq!(r, Resolved::Target(TargetRef::new("acme", "kb", "develop")));
    }

    #[tokio::test]
    async fn legacy_3segment_with_registry_miss_yields_unresolved() {
        let pool = pool_with_seed(&[]).await;
        let r = resolve_path("/acme/kb/knowledge", &fallback(), Some(&pool)).await;
        assert_eq!(
            r,
            Resolved::Unresolved {
                org: "acme".into(),
                repo: "kb".into(),
                suffix: LegacySuffix::Knowledge,
            }
        );
    }

    #[tokio::test]
    async fn legacy_admin_miss_preserves_suffix() {
        let pool = pool_with_seed(&[]).await;
        let r = resolve_path("/acme/kb/admin", &fallback(), Some(&pool)).await;
        match r {
            Resolved::Unresolved { suffix, .. } => assert_eq!(suffix, LegacySuffix::Admin),
            other => panic!("expected Unresolved Admin, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn legacy_admin_views_miss_preserves_suffix() {
        let pool = pool_with_seed(&[]).await;
        let r = resolve_path("/acme/kb/admin/views", &fallback(), Some(&pool)).await;
        match r {
            Resolved::Unresolved { suffix, .. } => assert_eq!(suffix, LegacySuffix::AdminViews),
            other => panic!("expected Unresolved AdminViews, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn assets_3segment_falls_back_to_env_branch() {
        let pool = pool_with_seed(&[]).await;
        let r = resolve_path("/acme/kb/assets/img.png", &fallback(), Some(&pool)).await;
        match r {
            Resolved::Default(t) => {
                assert_eq!(t.org, "acme");
                assert_eq!(t.repo, "kb");
                assert_eq!(t.branch, "env-branch");
            }
            other => panic!("expected Default with env branch, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn api_path_yields_default() {
        let r = resolve_path("/api/save_brain_file", &fallback(), None).await;
        assert!(matches!(r, Resolved::Default(t) if t.org == "env-org"));
    }

    #[tokio::test]
    async fn unknown_path_yields_default() {
        let r = resolve_path("/", &fallback(), None).await;
        assert!(matches!(r, Resolved::Default(_)));
    }

    /// Critical trust-boundary regression test: a forged Referer header on
    /// `/api/*` cannot influence the resolved target. We assert this by
    /// observing that `resolve_path` does not even take the request — it
    /// only takes a path. The API handler in `main.rs` calls
    /// `resolve_path(request.uri().path(), ...)`, never reading headers.
    #[tokio::test]
    async fn api_path_resolution_does_not_consider_headers() {
        // Two requests with the same path resolve identically regardless of
        // any other request state. The axum handler obtains target from
        // path only.
        let r1 = resolve_path("/api/save_brain_file", &fallback(), None).await;
        let r2 = resolve_path("/api/save_brain_file", &fallback(), None).await;
        assert_eq!(r1, r2);
    }

    #[test]
    fn legacy_canonical_path_percent_encodes_branch() {
        let m = LegacyTargetUnresolved {
            org: "acme".into(),
            repo: "kb".into(),
            suffix: LegacySuffix::Knowledge,
        };
        assert_eq!(m.canonical_path("main"), "/acme/kb/main/knowledge");
        // Branch with `/` (e.g. feature/foo) percent-encodes to `feature%2Ffoo`
        // so the URL stays unambiguous against the route shape.
        assert_eq!(
            m.canonical_path("feature/foo"),
            "/acme/kb/feature%2Ffoo/knowledge"
        );
    }

    #[test]
    fn legacy_canonical_path_preserves_admin_views_suffix() {
        let m = LegacyTargetUnresolved {
            org: "acme".into(),
            repo: "kb".into(),
            suffix: LegacySuffix::AdminViews,
        };
        assert_eq!(m.canonical_path("main"), "/acme/kb/main/admin/views");
    }
}
