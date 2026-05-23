//! Sticky branch registry for forge targets.
//!
//! When a request hits a 3-segment legacy URL like `/{org}/{repo}/knowledge`
//! the server needs to resolve which branch this deployment operates against
//! for that repo. Doing a `GET /repos/{owner}/{repo}` on every request would
//! be slow and would also let the resolved branch drift silently if the
//! upstream default changes — incompatible with the deterministic-stickiness
//! contract of 3.7B-α.
//!
//! The registry collapses both problems into one SQLite SELECT on the hot
//! path. The first time a `(org, repo)` pair is observed, `register_or_get`
//! discovers `default_branch` from the forge once, INSERTs a row with
//! `source = 'lazy_legacy'`, and returns the entry. Every subsequent request
//! finds the row and skips GitHub forever (until an explicit re-register —
//! a separate mutation in 3.7B-β).
//!
//! The underlying table is `targets`, which is also the FK parent of
//! `files`/`nodes`/`edges`/`projection_sync_state`. UNIQUE(org, repo) is
//! enforced at the schema level (see [`crate::server::projection::migrate`]).

use brain_domain::{BrainError, GithubClient, TargetConfig, TargetRef};
use brain_storage::GithubHttp;
use sqlx::SqlitePool;

/// One sticky branch registration for a `(org, repo)` pair on this deployment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryEntry {
    pub org: String,
    pub repo: String,
    pub branch: String,
    pub default_branch: Option<String>,
    pub source: String,
    pub registered_at: String,
    pub registered_by: Option<String>,
}

impl RegistryEntry {
    pub fn target_ref(&self) -> TargetRef {
        TargetRef::new(&self.org, &self.repo, &self.branch)
    }
}

type LookupRow = (
    String,
    String,
    String,
    Option<String>,
    String,
    String,
    Option<String>,
);

/// Read-only lookup. Returns `Ok(None)` when no row exists for `(org, repo)`.
/// Hot-path call — every request to a legacy 3-segment URL flows through here.
pub async fn lookup(
    pool: &SqlitePool,
    org: &str,
    repo: &str,
) -> Result<Option<RegistryEntry>, BrainError> {
    let row: Option<LookupRow> = sqlx::query_as(
        "SELECT org, repo, branch, default_branch, source, registered_at, registered_by
         FROM targets WHERE org = ? AND repo = ?",
    )
    .bind(org)
    .bind(repo)
    .fetch_optional(pool)
    .await
    .map_err(|e| BrainError::other(format!("target_registry lookup: {e}")))?;
    Ok(row.map(
        |(org, repo, branch, default_branch, source, registered_at, registered_by)| RegistryEntry {
            org,
            repo,
            branch,
            default_branch,
            source,
            registered_at,
            registered_by,
        },
    ))
}

/// Lookup-or-discover. On miss, call the forge once for `default_branch`,
/// `INSERT OR IGNORE` the row, then re-SELECT to return whichever entry
/// won the race (concurrent first-callers converge to one row; the loser
/// observes the winner's branch instead of its own resolved value, which
/// is correct because both reads of `default_branch` produced the same
/// answer in the same millisecond window).
///
/// `actor` is the session login that triggered the registration, recorded
/// for audit. `None` is acceptable when the trigger is non-user (webhook,
/// admin tooling).
pub async fn register_or_get(
    pool: &SqlitePool,
    http: &GithubHttp,
    token: &str,
    org: &str,
    repo: &str,
    actor: Option<&str>,
) -> Result<RegistryEntry, BrainError> {
    if let Some(existing) = lookup(pool, org, repo).await? {
        return Ok(existing);
    }
    let branch = discover_default_branch(http, token, org, repo).await?;
    sqlx::query(
        "INSERT OR IGNORE INTO targets (
            key, org, repo, branch, registered_at, default_branch, source, registered_by
         )
         VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP, ?, 'lazy_legacy', ?)",
    )
    .bind(format!("{org}/{repo}/{branch}"))
    .bind(org)
    .bind(repo)
    .bind(&branch)
    .bind(&branch)
    .bind(actor)
    .execute(pool)
    .await
    .map_err(|e| BrainError::other(format!("target_registry insert: {e}")))?;
    lookup(pool, org, repo)
        .await?
        .ok_or_else(|| BrainError::other("target_registry: row vanished after insert"))
}

/// Persist the forge-discovered default branch without changing the sticky
/// operational branch for an already-registered target.
pub async fn remember_default_branch(
    pool: &SqlitePool,
    org: &str,
    repo: &str,
    default_branch: &str,
    actor: Option<&str>,
) -> Result<(), BrainError> {
    sqlx::query(
        "INSERT OR IGNORE INTO targets (
            key, org, repo, branch, registered_at, default_branch, source, registered_by
         )
         VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP, ?, 'switcher_discovery', ?)",
    )
    .bind(format!("{org}/{repo}/{default_branch}"))
    .bind(org)
    .bind(repo)
    .bind(default_branch)
    .bind(default_branch)
    .bind(actor)
    .execute(pool)
    .await
    .map_err(|e| BrainError::other(format!("target_registry remember_default_branch: {e}")))?;
    sqlx::query("UPDATE targets SET default_branch = ? WHERE org = ? AND repo = ?")
        .bind(default_branch)
        .bind(org)
        .bind(repo)
        .execute(pool)
        .await
        .map_err(|e| BrainError::other(format!("target_registry update default_branch: {e}")))?;
    Ok(())
}

#[cfg(feature = "ssr")]
async fn discover_default_branch(
    http: &GithubHttp,
    token: &str,
    org: &str,
    repo: &str,
) -> Result<String, BrainError> {
    let client = GithubClient::new(TargetConfig {
        org: org.to_string(),
        repo: repo.to_string(),
        branch: String::new(),
    });
    let url = client.target_repo_url();
    #[derive(serde::Deserialize)]
    struct GhRepo {
        default_branch: String,
    }
    let response = http
        .get(&url, token)
        .send()
        .await
        .map_err(|e| BrainError::other(format!("discover default_branch: {e}")))?;
    if !response.status().is_success() {
        return Err(BrainError::other(format!(
            "discover default_branch for {org}/{repo}: HTTP {}",
            response.status()
        )));
    }
    let body: GhRepo = response
        .json()
        .await
        .map_err(|e| BrainError::other(format!("decode default_branch: {e}")))?;
    if body.default_branch.is_empty() {
        return Err(BrainError::other(format!(
            "default_branch empty for {org}/{repo}"
        )));
    }
    Ok(body.default_branch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;

    async fn test_pool() -> SqlitePool {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .unwrap();
        crate::server::projection::migrate(&pool).await.unwrap();
        pool
    }

    /// Pre-seed a row by reusing `ensure_target_id` — bypasses the network
    /// path that `register_or_get` would otherwise take. Lets us test
    /// `lookup` and the "row exists" branch of `register_or_get` without
    /// wiring a wiremock.
    async fn seed(pool: &SqlitePool, org: &str, repo: &str, branch: &str, source: &str) {
        sqlx::query(
            "INSERT INTO targets (key, org, repo, branch, source, registered_by)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(format!("{org}/{repo}/{branch}"))
        .bind(org)
        .bind(repo)
        .bind(branch)
        .bind(source)
        .bind(Option::<String>::None)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn lookup_returns_none_for_unknown_target() {
        let pool = test_pool().await;
        let result = lookup(&pool, "acme", "kb").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn lookup_returns_seeded_row() {
        let pool = test_pool().await;
        seed(&pool, "acme", "kb", "main", "lazy_legacy").await;
        let entry = lookup(&pool, "acme", "kb").await.unwrap().unwrap();
        assert_eq!(entry.org, "acme");
        assert_eq!(entry.repo, "kb");
        assert_eq!(entry.branch, "main");
        assert_eq!(entry.source, "lazy_legacy");
        assert!(!entry.registered_at.is_empty());
        assert_eq!(entry.target_ref(), TargetRef::new("acme", "kb", "main"));
    }

    #[tokio::test]
    async fn lookup_distinguishes_different_repos() {
        let pool = test_pool().await;
        seed(&pool, "acme", "kb", "main", "lazy_legacy").await;
        seed(&pool, "acme", "wiki", "develop", "lazy_legacy").await;
        let kb = lookup(&pool, "acme", "kb").await.unwrap().unwrap();
        let wiki = lookup(&pool, "acme", "wiki").await.unwrap().unwrap();
        assert_eq!(kb.branch, "main");
        assert_eq!(wiki.branch, "develop");
    }

    /// When a row already exists, `register_or_get` must return it without
    /// touching the network. We pass an unconfigured token / non-routable
    /// URL via the type system: `GithubHttp::new` builds a pooled client
    /// but the function should never actually call `discover_default_branch`
    /// on the existing-row path.
    #[tokio::test]
    async fn register_or_get_returns_existing_without_network() {
        let pool = test_pool().await;
        seed(&pool, "acme", "kb", "main", "lazy_legacy").await;
        let http = GithubHttp::new().unwrap();
        let entry = register_or_get(&pool, &http, "irrelevant-token", "acme", "kb", None)
            .await
            .unwrap();
        assert_eq!(entry.branch, "main");
        assert_eq!(entry.source, "lazy_legacy");
    }
}
