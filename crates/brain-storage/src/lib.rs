#![allow(async_fn_in_trait)]
//! I/O layer for the Brain GitHub repo.
//!
//! Owns:
//! - Shared `reqwest::Client` builder (user-agent + TLS).
//! - The `contents/{path}` URL builder.
//! - The live graph loader (tree walk + base64 decode + `brain-graph` delegation).
//! - The template loader.
//! - In-memory TTL caches for both.
//!
//! Returns typed `BrainError` values; callers adapt to their transport at the edge.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::Engine;
use brain_domain::{BrainError, Edge, GithubClient, Node, TargetConfig, TargetKey};
use brain_graph::{RawFile, build_graph, is_included_md};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub mod atomic_rename;
pub mod git_transaction;
pub use git_transaction::{
    BackoffPolicy, GitTransaction, GitTransactionOutcome, RenameMutation, RenameOutcome,
};

pub trait Storage: Send + Sync {
    async fn load_template(&self, token: &str, filename: &str) -> Result<String, BrainError>;
    async fn load_graph(
        &self,
        token: &str,
        config: &brain_domain::BrainConfig,
    ) -> Result<(Vec<Node>, Vec<Edge>), BrainError>;
    async fn read_file(&self, token: &str, path: &str) -> Result<(String, String), BrainError>;
    #[allow(clippy::too_many_arguments)]
    async fn save_file(
        &self,
        token: &str,
        path: &str,
        content: &str,
        sha: Option<&str>,
        message: &str,
        author_name: &str,
        author_email: &str,
    ) -> Result<String, BrainError>;
    async fn delete_file(
        &self,
        token: &str,
        path: &str,
        sha: &str,
        message: &str,
        author_name: &str,
        author_email: &str,
    ) -> Result<(), BrainError>;
    async fn create_folder(
        &self,
        token: &str,
        folder_path: &str,
        message: &str,
        author_name: &str,
        author_email: &str,
    ) -> Result<String, BrainError>;
    #[allow(clippy::too_many_arguments)]
    async fn upload_binary(
        &self,
        token: &str,
        path: &str,
        bytes: &[u8],
        message: &str,
        author_name: &str,
        author_email: &str,
    ) -> Result<String, BrainError>;
    async fn list_folders(&self, token: &str) -> Result<Vec<String>, BrainError>;
    fn invalidate_cache(&self);
}

/// Target-agnostic HTTP transport for the GitHub REST API. Wraps a shared
/// `reqwest::Client` and centralises `Authorization`, `User-Agent`, `Accept`,
/// and `X-GitHub-Api-Version` headers.
///
/// **Carries no target binding.** A single instance built at server startup is
/// safe to share across every target the process talks to; the per-call
/// `GithubClient` (URL builder) decides which repo to hit. This split is what
/// keeps Phase 3's multi-target switcher from accidentally reading the wrong
/// repository through a startup-bound transport.
#[derive(Clone)]
pub struct GithubHttp {
    inner: Arc<reqwest::Client>,
}

impl GithubHttp {
    /// Build a pooled, target-agnostic client. Call once at server startup;
    /// downstream callers clone the result.
    pub fn new() -> Result<Self, BrainError> {
        let inner = {
            let builder = reqwest::Client::builder().user_agent("brain_ui");
            #[cfg(not(target_arch = "wasm32"))]
            let builder = builder.timeout(Duration::from_secs(30));
            builder.build()
        }
        .map_err(|e| BrainError::Io(format!("http client: {e}")))?;
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    /// Shared `reqwest::Client` (cheap clone). Exposed for ad-hoc callers that
    /// need direct access — most code should use the header helpers below.
    pub fn client(&self) -> Arc<reqwest::Client> {
        self.inner.clone()
    }

    fn auth_headers(rb: reqwest::RequestBuilder, token: &str) -> reqwest::RequestBuilder {
        rb.bearer_auth(token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
    }

    pub fn get(&self, url: &str, token: &str) -> reqwest::RequestBuilder {
        Self::auth_headers(self.inner.get(url), token)
    }

    pub fn put(&self, url: &str, token: &str) -> reqwest::RequestBuilder {
        Self::auth_headers(self.inner.put(url), token)
    }

    pub fn delete(&self, url: &str, token: &str) -> reqwest::RequestBuilder {
        Self::auth_headers(self.inner.delete(url), token)
    }

    pub fn post(&self, url: &str, token: &str) -> reqwest::RequestBuilder {
        Self::auth_headers(self.inner.post(url), token)
    }

    pub fn patch(&self, url: &str, token: &str) -> reqwest::RequestBuilder {
        Self::auth_headers(self.inner.patch(url), token)
    }

    /// Send a request and decode JSON. Centralises the
    /// `.send().error_for_status().json()` chain. `ctx` is a short string used
    /// in error and warning messages so callers don't have to repeat it three
    /// times per call site.
    pub async fn send_json<T: DeserializeOwned>(
        rb: reqwest::RequestBuilder,
        ctx: &str,
    ) -> Result<T, BrainError> {
        let resp = rb
            .send()
            .await
            .map_err(|e| BrainError::github(format!("{ctx} fetch: {e}")))?;
        let status = resp.status();
        tracing::debug!(%status, ctx, "github response");
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            let snippet: String = body.chars().take(512).collect();
            tracing::warn!(%status, ctx, body = %snippet, "github non-success");
            return Err(BrainError::github(format!(
                "{ctx} status {status}: {snippet}"
            )));
        }
        resp.json::<T>()
            .await
            .map_err(|e| BrainError::github(format!("{ctx} parse: {e}")))
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct GithubIssuePatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignees: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GithubIssueComment {
    pub html_url: String,
    pub body: String,
    pub created_at: String,
    pub updated_at: String,
    pub user: GithubIssueCommentUser,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GithubIssueCommentUser {
    pub login: String,
    pub html_url: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct RepositoryPermissions {
    #[serde(default)]
    pub pull: bool,
    #[serde(default)]
    pub push: bool,
    #[serde(default)]
    pub admin: bool,
    #[serde(default)]
    pub maintain: bool,
    #[serde(default)]
    pub triage: bool,
}

#[derive(Clone, Debug)]
pub struct PullRequestOutcome {
    pub number: u64,
    pub html_url: String,
}

/// Combination of target-agnostic HTTP transport + target-bound URL builder.
/// Built per-call from whatever target the caller actually wants to talk to.
/// Use [`GithubStorage::new`] in production; its inputs make the target
/// binding explicit and prevent the "transport bound at startup" foot-gun.
pub struct GithubStorage {
    http: GithubHttp,
    gh: GithubClient,
}

impl GithubStorage {
    /// Build a storage from an existing pooled transport and an explicit
    /// target. This is the production constructor — the target is **always**
    /// the one the caller passed, never silently inherited from process-wide
    /// state.
    pub fn new(http: GithubHttp, target: TargetConfig) -> Self {
        Self {
            http,
            gh: GithubClient::new(target),
        }
    }

    /// Test-only convenience that builds its own transport. Production code
    /// should reuse the startup-built `GithubHttp` to keep connection pooling
    /// effective.
    pub fn standalone(target: TargetConfig) -> Self {
        let http = GithubHttp::new().expect("reqwest client");
        Self::new(http, target)
    }

    pub fn http(&self) -> &GithubHttp {
        &self.http
    }

    pub fn target(&self) -> &TargetConfig {
        self.gh.target()
    }

    fn contents_url(&self, path: &str) -> String {
        self.gh.contents_url(path)
    }

    fn get_contents(&self, token: &str, path: &str) -> reqwest::RequestBuilder {
        self.http
            .get(&self.contents_url(path), token)
            .query(&[("ref", self.branch())])
    }

    fn branch(&self) -> &str {
        &self.gh.target().branch
    }

    fn target_key(&self) -> TargetKey {
        TargetKey::from(self.gh.target())
    }

    pub async fn repository_permissions(
        &self,
        token: &str,
    ) -> Result<RepositoryPermissions, BrainError> {
        #[derive(Deserialize)]
        struct RepoResponse {
            #[serde(default)]
            permissions: RepositoryPermissions,
        }

        let url = self.gh.target_repo_url();
        let repo: RepoResponse = GithubHttp::send_json(self.http.get(&url, token), "repo").await?;
        Ok(repo.permissions)
    }

    pub async fn head_sha(&self, token: &str) -> Result<String, BrainError> {
        let ref_url = self.gh.git_ref_url();
        #[derive(Deserialize)]
        struct RefResponse {
            object: RefObject,
        }
        #[derive(Deserialize)]
        struct RefObject {
            sha: String,
        }
        let ref_resp: RefResponse =
            GithubHttp::send_json(self.http.get(&ref_url, token), "git_ref").await?;
        Ok(ref_resp.object.sha)
    }

    pub async fn create_branch_from_sha(
        &self,
        token: &str,
        branch: &str,
        sha: &str,
    ) -> Result<(), BrainError> {
        let url = self.gh.git_refs_url();
        let body = serde_json::json!({
            "ref": format!("refs/heads/{branch}"),
            "sha": sha,
        });
        let resp = self
            .http
            .post(&url, token)
            .json(&body)
            .send()
            .await
            .map_err(|e| BrainError::github(format!("git_ref_create fetch: {e}")))?;
        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }
        let body = resp.text().await.unwrap_or_default();
        Err(BrainError::github(format!(
            "git_ref_create status {status}: {}",
            body.chars().take(512).collect::<String>()
        )))
    }

    pub async fn ensure_fork(&self, token: &str, owner: &str) -> Result<(), BrainError> {
        let repo_url = self.gh.repo_url(owner, &self.gh.target().repo);
        let existing = self
            .http
            .get(&repo_url, token)
            .send()
            .await
            .map_err(|e| BrainError::github(format!("fork_repo_probe fetch: {e}")))?;
        if existing.status().is_success() {
            return Ok(());
        }

        let fork_url = self.gh.forks_url();
        let resp = self
            .http
            .post(&fork_url, token)
            .send()
            .await
            .map_err(|e| BrainError::github(format!("fork_create fetch: {e}")))?;
        let status = resp.status();
        if status.is_success() || status.as_u16() == 202 {
            return Ok(());
        }
        let body = resp.text().await.unwrap_or_default();
        Err(BrainError::github(format!(
            "fork_create status {status}: {}",
            body.chars().take(512).collect::<String>()
        )))
    }

    pub async fn open_pull_request(
        &self,
        token: &str,
        head: &str,
        base: &str,
        title: &str,
        body: &str,
    ) -> Result<PullRequestOutcome, BrainError> {
        #[derive(Deserialize)]
        struct PullResponse {
            number: u64,
            html_url: String,
        }

        let url = self.gh.pulls_url();
        let payload = serde_json::json!({
            "title": title,
            "head": head,
            "base": base,
            "body": body,
            "maintainer_can_modify": true,
        });
        let pr: PullResponse =
            GithubHttp::send_json(self.http.post(&url, token).json(&payload), "pull_create")
                .await?;
        Ok(PullRequestOutcome {
            number: pr.number,
            html_url: pr.html_url,
        })
    }

    /// Fetch every markdown file that participates in the Brain graph from the
    /// current target repository.
    pub async fn fetch_raw_files(&self, token: &str) -> Result<Vec<RawFile>, BrainError> {
        // Resolve the branch name to a commit SHA. GitHub's tree API works more
        // reliably with commit SHAs than branch names (avoids edge cases with
        // recently created / renamed branches or race conditions).
        let commit_sha = self.head_sha(token).await?;

        // Now fetch the tree using the resolved commit SHA instead of the branch name.
        let tree_url = self.gh.git_tree_by_sha_url(&commit_sha);
        let tree: TreeResponse =
            GithubHttp::send_json(self.http.get(&tree_url, token), "tree").await?;

        let mut candidates: Vec<String> = tree
            .tree
            .into_iter()
            .filter(|entry| entry.kind == "blob")
            .filter(|entry| is_included_md(&entry.path))
            .map(|entry| entry.path)
            .collect();
        candidates.sort();

        let mut files: Vec<RawFile> = Vec::with_capacity(candidates.len());
        for path in &candidates {
            // Propagate fetch errors so the caller never commits a partial
            // snapshot as "ready" when a transient GitHub error is the cause.
            let body: ContentResponse =
                GithubHttp::send_json(self.get_contents(token, path), "content").await?;
            let cleaned: String = body
                .content
                .chars()
                .filter(|ch| !ch.is_whitespace())
                .collect();
            let bytes = match base64::engine::general_purpose::STANDARD.decode(cleaned) {
                Ok(bytes) => bytes,
                Err(error) => {
                    tracing::warn!(path, error = %error, "base64 decode failed; skipping");
                    continue;
                }
            };
            let text = match String::from_utf8(bytes) {
                Ok(text) => text,
                Err(_) => {
                    tracing::warn!(path, "non-utf8 content; skipping");
                    continue;
                }
            };
            files.push(RawFile {
                path: path.clone(),
                sha: body.sha,
                content: text,
            });
        }

        Ok(files)
    }

    /// Commit a Git Data API transaction and invalidate the per-target graph
    /// cache on success so the next read reflects the new tree.
    pub async fn commit_transaction(
        &self,
        token: &str,
        transaction: GitTransaction,
    ) -> Result<GitTransactionOutcome, BrainError> {
        let outcome = transaction.commit(&self.http, &self.gh, token).await?;
        invalidate(&self.target_key());
        Ok(outcome)
    }

    /// Apply a rename as a single Git Data API commit. Kept as a compatibility
    /// wrapper while callers migrate to [`GitTransaction`].
    pub async fn atomic_rename(
        &self,
        token: &str,
        mutation: RenameMutation,
        policy: BackoffPolicy,
    ) -> Result<RenameOutcome, BrainError> {
        let transaction = GitTransaction::new(
            mutation.message,
            mutation.author_name,
            mutation.author_email,
        )
        .with_policy(policy);

        let transaction = mutation
            .upserts
            .into_iter()
            .fold(transaction, |tx, (path, content)| {
                tx.upsert_text(path, content)
            });
        let transaction = mutation
            .deletes
            .into_iter()
            .fold(transaction, |tx, path| tx.delete(path));
        let transaction = mutation
            .expect_absent
            .into_iter()
            .fold(transaction, |tx, path| tx.expect_absent(path));
        let transaction = mutation
            .expected_shas
            .into_iter()
            .fold(transaction, |tx, (path, sha)| tx.expect_sha(path, sha));

        self.commit_transaction(token, transaction)
            .await
            .map(Into::into)
    }

    /// Read the current label names for a GitHub issue. The sync layer uses
    /// this before replacing only Brain-managed state labels, preserving any
    /// unrelated labels maintained directly on GitHub.
    pub async fn issue_labels(
        &self,
        token: &str,
        project: &str,
        item_key: &str,
    ) -> Result<Vec<String>, BrainError> {
        #[derive(Deserialize)]
        struct IssueResponse {
            #[serde(default)]
            labels: Vec<IssueLabel>,
        }

        #[derive(Deserialize)]
        struct IssueLabel {
            name: String,
        }

        let url = self.gh.issue_url(project, item_key)?;
        let issue: IssueResponse =
            GithubHttp::send_json(self.http.get(&url, token), "issue_get").await?;
        Ok(issue.labels.into_iter().map(|label| label.name).collect())
    }

    /// Read issue comments for a GitHub-bound work item. This is deliberately
    /// a provider read, not part of the SQLite projection yet; callers can
    /// later cache the same shape behind a reconciliation job.
    pub async fn issue_comments(
        &self,
        token: &str,
        project: &str,
        item_key: &str,
    ) -> Result<Vec<GithubIssueComment>, BrainError> {
        let url = self.gh.issue_comments_url(project, item_key)?;
        GithubHttp::send_json(self.http.get(&url, token), "issue_comments").await
    }

    /// Patch a GitHub issue. `labels` replaces the issue label set when
    /// present, matching GitHub's REST API semantics; callers should read and
    /// merge existing labels first when they only own a subset.
    pub async fn patch_issue(
        &self,
        token: &str,
        project: &str,
        item_key: &str,
        patch: &GithubIssuePatch,
    ) -> Result<(), BrainError> {
        let url = self.gh.issue_url(project, item_key)?;
        let resp = self
            .http
            .patch(&url, token)
            .json(patch)
            .send()
            .await
            .map_err(|e| BrainError::github(format!("issue_patch fetch: {e}")))?;
        let status = resp.status();
        tracing::debug!(%status, project, item_key, "github issue patch response");
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            let snippet: String = body.chars().take(512).collect();
            tracing::warn!(%status, project, item_key, body = %snippet, "github issue patch failed");
            return Err(BrainError::github(format!(
                "issue_patch status {status}: {snippet}"
            )));
        }
        Ok(())
    }
}

// --- per-target caches ---------------------------------------------------

const CACHE_TTL: Duration = Duration::from_secs(30);
const TEMPLATE_TTL: Duration = Duration::from_secs(600);

struct CacheEntry {
    stored_at: Instant,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
}

struct TemplateEntry {
    stored_at: Instant,
    body: String,
}

fn graph_cache() -> &'static Mutex<HashMap<TargetKey, CacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<TargetKey, CacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn template_cache() -> &'static Mutex<HashMap<TargetKey, HashMap<String, TemplateEntry>>> {
    static CACHE: OnceLock<Mutex<HashMap<TargetKey, HashMap<String, TemplateEntry>>>> =
        OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn template_cache_get(key: &TargetKey, filename: &str) -> Option<String> {
    let mut guard = template_cache().lock().ok()?;
    let mut remove_target = false;
    let result = {
        let map = guard.get_mut(key)?;
        let expired = match map.get(filename) {
            Some(entry) => entry.stored_at.elapsed() > TEMPLATE_TTL,
            None => return None,
        };
        if expired {
            map.remove(filename);
            remove_target = map.is_empty();
            None
        } else {
            map.get(filename).map(|entry| entry.body.clone())
        }
    };
    if remove_target {
        guard.remove(key);
    }
    result
}

fn template_cache_store(key: &TargetKey, filename: &str, body: &str) {
    if let Ok(mut guard) = template_cache().lock() {
        let map = guard.entry(key.clone()).or_default();
        map.insert(
            filename.to_string(),
            TemplateEntry {
                stored_at: Instant::now(),
                body: body.to_string(),
            },
        );
    }
}

/// Drop the graph cache for a single target. Called from every successful
/// write path and from the manual `RefreshGraph` server fn.
pub fn invalidate(key: &TargetKey) {
    if let Ok(mut guard) = graph_cache().lock() {
        guard.remove(key);
    }
}

/// Drop the template cache for a single target. Templates rarely change but
/// the cache had no invalidation at all before; the manual refresh path uses
/// this to force a clean reload.
pub fn invalidate_template(key: &TargetKey) {
    if let Ok(mut guard) = template_cache().lock() {
        guard.remove(key);
    }
}

fn cache_get(key: &TargetKey) -> Option<(Vec<Node>, Vec<Edge>)> {
    let mut guard = graph_cache().lock().ok()?;
    let expired = guard
        .get(key)
        .map(|e| e.stored_at.elapsed() > CACHE_TTL)
        .unwrap_or(true);
    if expired {
        guard.remove(key);
        return None;
    }
    guard.get(key).map(|e| (e.nodes.clone(), e.edges.clone()))
}

fn cache_store(key: &TargetKey, nodes: &[Node], edges: &[Edge]) {
    if let Ok(mut guard) = graph_cache().lock() {
        guard.insert(
            key.clone(),
            CacheEntry {
                stored_at: Instant::now(),
                nodes: nodes.to_vec(),
                edges: edges.to_vec(),
            },
        );
    }
}

#[derive(Deserialize)]
struct TreeResponse {
    tree: Vec<TreeEntry>,
}

#[derive(Deserialize)]
struct TreeEntry {
    path: String,
    #[serde(rename = "type")]
    kind: String,
    #[allow(dead_code)]
    sha: String,
}

#[derive(Deserialize)]
pub struct ContentResponse {
    pub content: String,
    pub sha: String,
}

#[derive(Deserialize)]
pub struct GhDirEntry {
    pub path: String,
    #[serde(rename = "type")]
    pub kind: String,
}

/// Build a fresh, **unpooled** `reqwest::Client`. Retained only for ad-hoc
/// callers (e.g. one-off scripts) that don't have a `GithubHttp` in scope.
/// New code should hold or borrow `GithubHttp::client()` instead.
#[deprecated(note = "use GithubHttp::new().client() and share the pooled client")]
pub fn http_client() -> Result<reqwest::Client, BrainError> {
    reqwest::Client::builder()
        .user_agent("brain_ui")
        .build()
        .map_err(|e| BrainError::Io(format!("http client: {e}")))
}

impl Storage for GithubStorage {
    async fn load_template(&self, token: &str, filename: &str) -> Result<String, BrainError> {
        let key = self.target_key();
        if let Some(hit) = template_cache_get(&key, filename) {
            return Ok(hit);
        }
        let path = format!("templates/{filename}");
        let body: ContentResponse =
            GithubHttp::send_json(self.get_contents(token, &path), "template").await?;
        let cleaned: String = body
            .content
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(cleaned)
            .map_err(|e| BrainError::parse(format!("template b64: {e}")))?;
        let text = String::from_utf8(bytes)
            .map_err(|e| BrainError::parse(format!("template utf8: {e}")))?;
        template_cache_store(&key, filename, &text);
        Ok(text)
    }

    async fn load_graph(
        &self,
        token: &str,
        config: &brain_domain::BrainConfig,
    ) -> Result<(Vec<Node>, Vec<Edge>), BrainError> {
        let key = self.target_key();
        if let Some(hit) = cache_get(&key) {
            return Ok(hit);
        }
        let files = self.fetch_raw_files(token).await?;
        let (nodes, edges) = build_graph(&files, config);
        cache_store(&key, &nodes, &edges);
        Ok((nodes, edges))
    }

    async fn read_file(&self, token: &str, path: &str) -> Result<(String, String), BrainError> {
        let resp: ContentResponse =
            GithubHttp::send_json(self.get_contents(token, path), "content").await?;

        let cleaned: String = resp
            .content
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(cleaned)
            .map_err(|e| BrainError::parse(format!("b64: {e}")))?;
        let text = String::from_utf8(bytes).map_err(|e| BrainError::parse(format!("utf8: {e}")))?;

        Ok((text, resp.sha))
    }

    #[allow(clippy::too_many_arguments)]
    async fn save_file(
        &self,
        token: &str,
        path: &str,
        content: &str,
        sha: Option<&str>,
        message: &str,
        author_name: &str,
        author_email: &str,
    ) -> Result<String, BrainError> {
        let mut transaction =
            GitTransaction::new(message, author_name, author_email).upsert_text(path, content);
        if let Some(s) = sha.filter(|s| !s.is_empty()) {
            transaction = transaction.expect_sha(path, s);
        } else {
            transaction = transaction.expect_absent(path);
        }
        self.commit_transaction(token, transaction).await?;
        Ok(path.to_string())
    }

    async fn delete_file(
        &self,
        token: &str,
        path: &str,
        sha: &str,
        message: &str,
        author_name: &str,
        author_email: &str,
    ) -> Result<(), BrainError> {
        let transaction = GitTransaction::new(message, author_name, author_email)
            .delete(path)
            .expect_sha(path, sha);
        self.commit_transaction(token, transaction).await?;
        Ok(())
    }

    async fn create_folder(
        &self,
        token: &str,
        folder_path: &str,
        message: &str,
        author_name: &str,
        author_email: &str,
    ) -> Result<String, BrainError> {
        let folder_title = folder_path.rsplit('/').next().unwrap_or(folder_path);
        let readme_content = format!("# {folder_title}\n\n(Section created via Brain UI)\n");
        let file_path = format!("{folder_path}/README.md");

        self.save_file(
            token,
            &file_path,
            &readme_content,
            None,
            message,
            author_name,
            author_email,
        )
        .await
    }

    async fn upload_binary(
        &self,
        token: &str,
        path: &str,
        bytes: &[u8],
        message: &str,
        author_name: &str,
        author_email: &str,
    ) -> Result<String, BrainError> {
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        let body = serde_json::json!({
            "message": message,
            "content": encoded,
            "branch": self.branch(),
            "committer": {
                "name": author_name,
                "email": author_email,
            }
        });
        let url = self.contents_url(path);
        let response = self
            .http
            .put(&url, token)
            .json(&body)
            .send()
            .await
            .map_err(|e| BrainError::github(format!("PUT: {e}")))?;
        if response.status().is_success() {
            // An uploaded asset doesn't change the markdown graph but it _can_
            // appear in `![alt](...)` markdown that we just rendered; bumping
            // the cache here is cheap insurance against stale image links and
            // closes the gap that was a latent bug pre-Phase-2A.
            invalidate(&self.target_key());
            Ok(path.to_string())
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(BrainError::github(format!(
                "API error {} — {}",
                status,
                body.chars().take(200).collect::<String>()
            )))
        }
    }

    async fn list_folders(&self, token: &str) -> Result<Vec<String>, BrainError> {
        let items: Vec<GhDirEntry> =
            GithubHttp::send_json(self.get_contents(token, ""), "list_folders").await?;

        let folders: Vec<String> = items
            .iter()
            .filter(|item| item.kind == "dir")
            .map(|item| item.path.clone())
            .collect();

        Ok(folders)
    }

    fn invalidate_cache(&self) {
        invalidate(&self.target_key());
    }
}

pub struct InMemoryStorage;

impl InMemoryStorage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl Storage for InMemoryStorage {
    async fn load_template(&self, _token: &str, _filename: &str) -> Result<String, BrainError> {
        Ok("".to_string())
    }

    async fn load_graph(
        &self,
        _token: &str,
        _config: &brain_domain::BrainConfig,
    ) -> Result<(Vec<Node>, Vec<Edge>), BrainError> {
        Ok((Vec::new(), Vec::new()))
    }

    async fn read_file(&self, _token: &str, _path: &str) -> Result<(String, String), BrainError> {
        Ok(("".to_string(), "".to_string()))
    }

    #[allow(clippy::too_many_arguments)]
    async fn save_file(
        &self,
        _token: &str,
        path: &str,
        _content: &str,
        _sha: Option<&str>,
        _message: &str,
        _author_name: &str,
        _author_email: &str,
    ) -> Result<String, BrainError> {
        Ok(path.to_string())
    }

    async fn delete_file(
        &self,
        _token: &str,
        _path: &str,
        _sha: &str,
        _message: &str,
        _author_name: &str,
        _author_email: &str,
    ) -> Result<(), BrainError> {
        Ok(())
    }

    async fn create_folder(
        &self,
        _token: &str,
        folder_path: &str,
        _message: &str,
        _author_name: &str,
        _author_email: &str,
    ) -> Result<String, BrainError> {
        Ok(format!("{folder_path}/README.md"))
    }

    async fn upload_binary(
        &self,
        _token: &str,
        path: &str,
        _bytes: &[u8],
        _message: &str,
        _author_name: &str,
        _author_email: &str,
    ) -> Result<String, BrainError> {
        Ok(path.to_string())
    }

    async fn list_folders(&self, _token: &str) -> Result<Vec<String>, BrainError> {
        Ok(Vec::new())
    }

    fn invalidate_cache(&self) {}
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    use base64::Engine;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn target(org: &str, repo: &str, branch: &str) -> TargetConfig {
        TargetConfig {
            org: org.into(),
            repo: repo.into(),
            branch: branch.into(),
        }
    }

    // Each test uses a unique repo name so the shared static cache map can't
    // cause cross-test interference under `cargo test`'s default parallel
    // executor. `invalidate_all()` would force serialisation, which is more
    // brittle than just picking distinct keys.
    #[test]
    fn graph_cache_isolates_targets() {
        let a = TargetKey::from(&target("o", "graph_iso_a", "main"));
        let b = TargetKey::from(&target("o", "graph_iso_b", "main"));
        invalidate(&a);
        invalidate(&b);

        cache_store(&a, &[], &[]);
        assert!(cache_get(&a).is_some(), "target A populated");
        assert!(cache_get(&b).is_none(), "target B must not see A's entry");

        invalidate(&a);
        assert!(cache_get(&a).is_none(), "after invalidate(A) A is empty");
    }

    #[test]
    fn template_cache_isolates_targets() {
        let a = TargetKey::from(&target("o", "tpl_iso_a", "main"));
        let b = TargetKey::from(&target("o", "tpl_iso_b", "main"));
        invalidate_template(&a);
        invalidate_template(&b);

        template_cache_store(&a, "ConceptNote.md", "BODY-A");
        assert_eq!(
            template_cache_get(&a, "ConceptNote.md").as_deref(),
            Some("BODY-A")
        );
        assert!(template_cache_get(&b, "ConceptNote.md").is_none());

        invalidate_template(&a);
        assert!(template_cache_get(&a, "ConceptNote.md").is_none());
    }

    #[test]
    fn invalidate_only_affects_named_target() {
        let a = TargetKey::from(&target("o", "inv_one_a", "main"));
        let b = TargetKey::from(&target("o", "inv_one_b", "main"));
        invalidate(&a);
        invalidate(&b);

        cache_store(&a, &[], &[]);
        cache_store(&b, &[], &[]);
        invalidate(&a);
        assert!(cache_get(&a).is_none());
        assert!(cache_get(&b).is_some(), "invalidating A must not touch B");

        invalidate(&b); // cleanup so re-runs of this test are deterministic
    }

    #[test]
    fn pooled_client_is_shared_across_targets() {
        // The pooled HTTP transport is target-agnostic: the same `GithubHttp`
        // backs storages for entirely different repos. This is the property
        // that lets Phase 3's multi-target switcher reuse one connection pool
        // across every target the user navigates to.
        let http = GithubHttp::new().expect("client");
        let storage_a = GithubStorage::new(http.clone(), target("o", "a", "main"));
        let storage_b = GithubStorage::new(http.clone(), target("o", "b", "main"));
        assert!(
            Arc::ptr_eq(&storage_a.http.inner, &storage_b.http.inner),
            "storages must share the underlying reqwest::Client even across targets"
        );
        assert_ne!(storage_a.target_key(), storage_b.target_key());
    }

    #[test]
    fn storage_url_uses_constructor_target_not_transport_default() {
        // Regression for the "transport silently re-bound to startup target"
        // class of bug: the URL must come from the target passed at storage
        // construction, regardless of any other GithubHttp around.
        let http = GithubHttp::new().expect("client");
        let storage = GithubStorage::new(http, target("acme", "knowledge", "main"));
        let url = storage.contents_url("notes/a.md");
        assert!(
            url.contains("/repos/acme/knowledge/contents/"),
            "url must come from the constructor target, got: {url}"
        );
    }

    #[tokio::test]
    async fn read_file_encodes_content_path_and_ref_query() {
        let server = MockServer::start().await;
        let encoded = base64::engine::general_purpose::STANDARD.encode("hello");
        Mock::given(method("GET"))
            .and(path(
                "/repos/acme/knowledge/contents/notes/space%20name/%23draft%3F.md",
            ))
            .and(query_param("ref", "feature/foo #1"))
            .and(header("authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "content": encoded,
                "sha": "abc123"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let http = GithubHttp::new().expect("client");
        let storage = GithubStorage {
            http,
            gh: GithubClient::new(target("acme", "knowledge", "feature/foo #1"))
                .with_api_base(server.uri()),
        };

        let (body, sha) = storage
            .read_file("test-token", "notes/space name/#draft?.md")
            .await
            .expect("read file");

        assert_eq!(body, "hello");
        assert_eq!(sha, "abc123");
    }

    #[tokio::test]
    async fn read_file_keeps_dot_segments_under_contents_endpoint() {
        let server = MockServer::start().await;
        let encoded = base64::engine::general_purpose::STANDARD.encode("hello");
        Mock::given(method("GET"))
            .and(path(
                "/repos/acme/knowledge/contents/%252E%252E/branches/main",
            ))
            .and(query_param("ref", "main"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "content": encoded,
                "sha": "abc123"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let http = GithubHttp::new().expect("client");
        let storage = GithubStorage {
            http,
            gh: GithubClient::new(target("acme", "knowledge", "main")).with_api_base(server.uri()),
        };

        let (body, sha) = storage
            .read_file("test-token", "../branches/main")
            .await
            .expect("read file");

        assert_eq!(body, "hello");
        assert_eq!(sha, "abc123");
    }

    #[test]
    fn target_key_format_is_org_repo_branch() {
        let k = TargetKey::from(&target("dritara", "brain", "main"));
        assert_eq!(k.as_str(), "dritara/brain/main");
    }
}
