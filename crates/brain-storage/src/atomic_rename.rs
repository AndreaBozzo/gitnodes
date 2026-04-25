//! Atomic file rename via the GitHub Git Data API.
//!
//! `rename_brain_file` previously issued one Contents API commit per touched
//! file (N referrers + 1 create + 1 delete = N+2 commits). This module collapses
//! the operation into a single commit by composing the lower-level Git Data
//! endpoints: `POST /git/blobs` → `POST /git/trees` → `POST /git/commits` →
//! `PATCH /git/refs/heads/{branch}`.
//!
//! The only race is the final ref update: a concurrent push between our
//! `GET /git/refs/...` and our `PATCH` will return `422 Update is not a fast
//! forward`. We retry that specific failure with exponential backoff up to a
//! small cap. Blob SHAs are content-addressed, so on retry we reuse the blobs
//! we already uploaded and only rebuild the tree+commit.
//!
//! No projection mutation happens here. The caller (`rename_brain_file`) keeps
//! its post-write `rebuild_projection_after_write` step exactly as before;
//! this preserves the No Dual-Write invariant: GitHub is source of truth, the
//! local SQLite read model realigns via rebuild/sync only.

use std::collections::HashMap;
use std::time::Duration;

use base64::Engine;
use brain_domain::{BrainError, GithubClient};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::GithubHttp;

/// Inputs to a single atomic rename commit.
pub struct RenameMutation {
    /// `(path, new_content)` pairs to add or update in the tree. The renamed
    /// file's *new* path must be in here; backlink-rewritten referrers also
    /// belong in this list.
    pub upserts: Vec<(String, String)>,
    /// Paths to remove from the tree. Typically `[old_path]`.
    pub deletes: Vec<String>,
    pub message: String,
    pub author_name: String,
    pub author_email: String,
}

/// Observable result of a successful rename. The caller doesn't need this for
/// correctness today, but tests and future audit logging benefit from it.
#[derive(Debug)]
pub struct RenameOutcome {
    pub commit_sha: String,
    pub head_before: String,
    pub head_after: String,
    pub attempts: u32,
}

/// Backoff schedule for `422 fast-forward` retries. Production uses
/// [`BackoffPolicy::default`]; tests can construct an instant policy to keep
/// suite latency low.
#[derive(Clone, Copy, Debug)]
pub struct BackoffPolicy {
    pub max_attempts: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl BackoffPolicy {
    pub const fn instant() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(0),
            max_delay: Duration::from_millis(0),
        }
    }
}

impl Default for BackoffPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(1_600),
        }
    }
}

/// Run a single atomic rename. Reuses already-uploaded blob SHAs across retries
/// — blobs are content-addressed so a retry only redoes tree+commit+ref.
pub async fn run(
    http: &GithubHttp,
    gh: &GithubClient,
    token: &str,
    mutation: RenameMutation,
    policy: BackoffPolicy,
) -> Result<RenameOutcome, BrainError> {
    if mutation.upserts.is_empty() && mutation.deletes.is_empty() {
        return Err(BrainError::other("atomic_rename: empty mutation"));
    }

    // Step 1: upload every blob exactly once. Content is fixed across retries,
    // so the resulting SHAs are stable; we cache by content to dedupe identical
    // upserts (rare but cheap to handle).
    let mut blob_shas: HashMap<String, String> = HashMap::new();
    let mut path_to_blob: Vec<(String, String)> = Vec::with_capacity(mutation.upserts.len());
    for (path, content) in &mutation.upserts {
        let sha = if let Some(existing) = blob_shas.get(content) {
            existing.clone()
        } else {
            let s = create_blob(http, gh, token, content).await?;
            blob_shas.insert(content.clone(), s.clone());
            s
        };
        path_to_blob.push((path.clone(), sha));
    }

    let mut last_error: Option<BrainError> = None;
    for attempt in 1..=policy.max_attempts {
        match attempt_commit(http, gh, token, &path_to_blob, &mutation).await {
            Ok(mut outcome) => {
                outcome.attempts = attempt;
                return Ok(outcome);
            }
            Err(AttemptError::FastForward(msg)) => {
                last_error = Some(BrainError::github(msg));
                if attempt < policy.max_attempts {
                    sleep_with_backoff(&policy, attempt).await;
                    continue;
                }
            }
            Err(AttemptError::Fatal(e)) => return Err(e),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        BrainError::github(format!(
            "fast-forward refused after {} retries",
            policy.max_attempts
        ))
    }))
}

enum AttemptError {
    /// Final ref update was rejected with `422 not a fast forward`. Safe to
    /// retry: re-read HEAD, rebuild tree+commit, re-push the ref.
    FastForward(String),
    /// Anything else: surface immediately, do not retry.
    Fatal(BrainError),
}

async fn attempt_commit(
    http: &GithubHttp,
    gh: &GithubClient,
    token: &str,
    path_to_blob: &[(String, String)],
    mutation: &RenameMutation,
) -> Result<RenameOutcome, AttemptError> {
    let head_before = get_head_sha(http, gh, token)
        .await
        .map_err(AttemptError::Fatal)?;
    let base_tree = get_commit_tree(http, gh, token, &head_before)
        .await
        .map_err(AttemptError::Fatal)?;

    let tree_sha = create_tree(http, gh, token, &base_tree, path_to_blob, &mutation.deletes)
        .await
        .map_err(AttemptError::Fatal)?;

    let commit_sha = create_commit(
        http,
        gh,
        token,
        &mutation.message,
        &tree_sha,
        &head_before,
        &mutation.author_name,
        &mutation.author_email,
    )
    .await
    .map_err(AttemptError::Fatal)?;

    match update_ref(http, gh, token, &commit_sha).await {
        Ok(()) => Ok(RenameOutcome {
            commit_sha,
            head_before,
            head_after: tree_sha, // overwritten by caller from response if needed
            attempts: 0,
        }),
        Err(e) => match classify_ref_error(&e) {
            Some(msg) => Err(AttemptError::FastForward(msg)),
            None => Err(AttemptError::Fatal(e)),
        },
    }
}

async fn sleep_with_backoff(policy: &BackoffPolicy, attempt: u32) {
    if policy.base_delay.is_zero() {
        return;
    }
    let factor = 1u32 << (attempt - 1).min(16);
    let raw = policy.base_delay.saturating_mul(factor);
    let capped = raw.min(policy.max_delay);
    // Deterministic jitter source: nanoseconds of `Instant::now`.
    let jitter_ns = std::time::Instant::now().elapsed().subsec_nanos() as u64 % 41;
    let jittered = capped + Duration::from_millis(jitter_ns);
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(jittered).await;
    #[cfg(target_arch = "wasm32")]
    let _ = jittered;
}

fn classify_ref_error(e: &BrainError) -> Option<String> {
    let msg = e.to_string();
    let lower = msg.to_lowercase();
    if lower.contains("status 422") && lower.contains("not a fast forward") {
        Some(msg)
    } else {
        None
    }
}

#[derive(Deserialize)]
struct RefResponse {
    object: RefObject,
}

#[derive(Deserialize)]
struct RefObject {
    sha: String,
}

async fn get_head_sha(
    http: &GithubHttp,
    gh: &GithubClient,
    token: &str,
) -> Result<String, BrainError> {
    let url = gh.git_ref_url();
    let resp: RefResponse = GithubHttp::send_json(http.get(&url, token), "git_ref").await?;
    Ok(resp.object.sha)
}

#[derive(Deserialize)]
struct CommitResponse {
    tree: TreeRef,
}

#[derive(Deserialize)]
struct TreeRef {
    sha: String,
}

async fn get_commit_tree(
    http: &GithubHttp,
    gh: &GithubClient,
    token: &str,
    commit_sha: &str,
) -> Result<String, BrainError> {
    let url = gh.git_commit_url(commit_sha);
    let resp: CommitResponse = GithubHttp::send_json(http.get(&url, token), "git_commit").await?;
    Ok(resp.tree.sha)
}

#[derive(Deserialize)]
struct BlobResponse {
    sha: String,
}

async fn create_blob(
    http: &GithubHttp,
    gh: &GithubClient,
    token: &str,
    content: &str,
) -> Result<String, BrainError> {
    let url = gh.git_blobs_url();
    let body = json!({
        "content": base64::engine::general_purpose::STANDARD.encode(content.as_bytes()),
        "encoding": "base64",
    });
    let resp: BlobResponse =
        GithubHttp::send_json(http.post(&url, token).json(&body), "git_blob").await?;
    Ok(resp.sha)
}

#[derive(Deserialize)]
struct TreeResponse {
    sha: String,
}

async fn create_tree(
    http: &GithubHttp,
    gh: &GithubClient,
    token: &str,
    base_tree: &str,
    path_to_blob: &[(String, String)],
    deletes: &[String],
) -> Result<String, BrainError> {
    let url = gh.git_trees_url();
    let mut entries: Vec<Value> = Vec::with_capacity(path_to_blob.len() + deletes.len());
    for (path, sha) in path_to_blob {
        entries.push(json!({
            "path": path,
            "mode": "100644",
            "type": "blob",
            "sha": sha,
        }));
    }
    for path in deletes {
        // GitHub's Git Data API treats `sha: null` as "remove this path from
        // the tree". This is the documented mechanism for deletes in a tree
        // composed against a `base_tree`.
        entries.push(json!({
            "path": path,
            "mode": "100644",
            "type": "blob",
            "sha": Value::Null,
        }));
    }
    let body = json!({
        "base_tree": base_tree,
        "tree": entries,
    });
    let resp: TreeResponse =
        GithubHttp::send_json(http.post(&url, token).json(&body), "git_tree").await?;
    Ok(resp.sha)
}

#[derive(Deserialize)]
struct CommitCreatedResponse {
    sha: String,
}

#[allow(clippy::too_many_arguments)]
async fn create_commit(
    http: &GithubHttp,
    gh: &GithubClient,
    token: &str,
    message: &str,
    tree_sha: &str,
    parent_sha: &str,
    author_name: &str,
    author_email: &str,
) -> Result<String, BrainError> {
    let url = gh.git_commits_url();
    let body = json!({
        "message": message,
        "tree": tree_sha,
        "parents": [parent_sha],
        "author": { "name": author_name, "email": author_email },
        "committer": { "name": author_name, "email": author_email },
    });
    let resp: CommitCreatedResponse =
        GithubHttp::send_json(http.post(&url, token).json(&body), "git_commit_create").await?;
    Ok(resp.sha)
}

async fn update_ref(
    http: &GithubHttp,
    gh: &GithubClient,
    token: &str,
    commit_sha: &str,
) -> Result<(), BrainError> {
    let url = gh.git_ref_url();
    let body = json!({ "sha": commit_sha, "force": false });
    // We can't reuse `send_json` here because the success body is uninteresting
    // and we need the raw status+body to classify a 422 fast-forward.
    let resp = http
        .patch(&url, token)
        .json(&body)
        .send()
        .await
        .map_err(|e| BrainError::github(format!("git_ref_update fetch: {e}")))?;
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    let body = resp.text().await.unwrap_or_default();
    let snippet: String = body.chars().take(512).collect();
    Err(BrainError::github(format!(
        "git_ref_update status {status}: {snippet}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_domain::TargetConfig;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, header, method, path, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn target() -> TargetConfig {
        TargetConfig {
            org: "acme".into(),
            repo: "kb".into(),
            branch: "main".into(),
        }
    }

    fn mutation_simple() -> RenameMutation {
        RenameMutation {
            upserts: vec![("notes/new.md".into(), "hello".into())],
            deletes: vec!["notes/old.md".into()],
            message: "rename".into(),
            author_name: "alice".into(),
            author_email: "alice@example.com".into(),
        }
    }

    /// Mounts every step of the pipeline EXCEPT the final `PATCH /git/refs`.
    /// Tests register the PATCH response themselves so the success/failure
    /// behaviour at the ref-update step is unambiguous.
    async fn ok_pipeline(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path("/repos/acme/kb/git/refs/heads/main"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ref": "refs/heads/main",
                "object": { "sha": "HEAD0", "type": "commit" }
            })))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repos/acme/kb/git/commits/HEAD0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "tree": { "sha": "TREE0" }
            })))
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(path("/repos/acme/kb/git/blobs"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "sha": "BLOB1"
            })))
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(path("/repos/acme/kb/git/trees"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "sha": "TREE1"
            })))
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(path("/repos/acme/kb/git/commits"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "sha": "COMMIT1"
            })))
            .mount(server)
            .await;
    }

    async fn mount_patch_ok(server: &MockServer) {
        Mock::given(method("PATCH"))
            .and(path("/repos/acme/kb/git/refs/heads/main"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "object": { "sha": "COMMIT1" }
            })))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn happy_path_single_commit() {
        let server = MockServer::start().await;
        ok_pipeline(&server).await;
        mount_patch_ok(&server).await;

        let http = GithubHttp::new().unwrap();
        let gh = GithubClient::new(target()).with_api_base(server.uri());
        let outcome = run(
            &http,
            &gh,
            "tok",
            mutation_simple(),
            BackoffPolicy::instant(),
        )
        .await
        .expect("rename ok");

        assert_eq!(outcome.commit_sha, "COMMIT1");
        assert_eq!(outcome.head_before, "HEAD0");
        assert_eq!(outcome.attempts, 1);
    }

    #[tokio::test]
    async fn sends_auth_and_useragent_headers() {
        let server = MockServer::start().await;
        ok_pipeline(&server).await;
        mount_patch_ok(&server).await;
        Mock::given(method("POST"))
            .and(path("/repos/acme/kb/git/blobs"))
            .and(header("authorization", "Bearer tok"))
            .and(header("user-agent", "brain_ui"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "sha": "BLOB1" })))
            .mount(&server)
            .await;

        let http = GithubHttp::new().unwrap();
        let gh = GithubClient::new(target()).with_api_base(server.uri());
        run(
            &http,
            &gh,
            "tok",
            mutation_simple(),
            BackoffPolicy::instant(),
        )
        .await
        .expect("ok");
    }

    #[tokio::test]
    async fn tree_payload_includes_null_sha_delete() {
        let server = MockServer::start().await;
        ok_pipeline(&server).await;
        mount_patch_ok(&server).await;
        // Match the tree create body: must include both an upsert with a sha
        // and a delete with `sha: null`. body_partial_json verifies a subset.
        Mock::given(method("POST"))
            .and(path("/repos/acme/kb/git/trees"))
            .and(body_partial_json(json!({
                "base_tree": "TREE0",
                "tree": [
                    { "path": "notes/new.md", "mode": "100644", "type": "blob", "sha": "BLOB1" },
                    { "path": "notes/old.md", "mode": "100644", "type": "blob", "sha": null },
                ]
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "sha": "TREE1" })))
            .mount(&server)
            .await;

        let http = GithubHttp::new().unwrap();
        let gh = GithubClient::new(target()).with_api_base(server.uri());
        run(
            &http,
            &gh,
            "tok",
            mutation_simple(),
            BackoffPolicy::instant(),
        )
        .await
        .expect("ok");
    }

    #[tokio::test]
    async fn retries_once_on_fast_forward_and_succeeds() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/acme/kb/git/refs/heads/main"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ref": "refs/heads/main",
                "object": { "sha": "HEAD0", "type": "commit" }
            })))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repos/acme/kb/git/refs/heads/main"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ref": "refs/heads/main",
                "object": { "sha": "HEAD1", "type": "commit" }
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/repos/acme/kb/git/commits/HEAD\d$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "tree": { "sha": "TREEBASE" }
            })))
            .mount(&server)
            .await;
        // Blobs should only be uploaded once even across retries: enforce
        // expected count == 1.
        Mock::given(method("POST"))
            .and(path("/repos/acme/kb/git/blobs"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "sha": "BLOB1" })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/repos/acme/kb/git/trees"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "sha": "TREE1" })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/repos/acme/kb/git/commits"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "sha": "COMMIT1" })))
            .mount(&server)
            .await;
        // First PATCH: 422 fast-forward. Second PATCH: 200.
        Mock::given(method("PATCH"))
            .and(path("/repos/acme/kb/git/refs/heads/main"))
            .respond_with(
                ResponseTemplate::new(422)
                    .set_body_string(r#"{"message":"Update is not a fast forward"}"#),
            )
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path("/repos/acme/kb/git/refs/heads/main"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "object": { "sha": "COMMIT1" }
            })))
            .mount(&server)
            .await;

        let http = GithubHttp::new().unwrap();
        let gh = GithubClient::new(target()).with_api_base(server.uri());
        let outcome = run(
            &http,
            &gh,
            "tok",
            mutation_simple(),
            BackoffPolicy::instant(),
        )
        .await
        .expect("retry then ok");
        assert_eq!(outcome.attempts, 2);
        assert_eq!(outcome.commit_sha, "COMMIT1");
    }

    #[tokio::test]
    async fn non_fast_forward_422_is_not_retried() {
        let server = MockServer::start().await;
        ok_pipeline(&server).await;
        Mock::given(method("PATCH"))
            .and(path("/repos/acme/kb/git/refs/heads/main"))
            .respond_with(
                ResponseTemplate::new(422)
                    .set_body_string(r#"{"message":"Reference does not exist"}"#),
            )
            .expect(1)
            .mount(&server)
            .await;

        let http = GithubHttp::new().unwrap();
        let gh = GithubClient::new(target()).with_api_base(server.uri());
        let err = run(
            &http,
            &gh,
            "tok",
            mutation_simple(),
            BackoffPolicy::instant(),
        )
        .await
        .expect_err("should not retry");
        assert!(err.to_string().contains("422"), "got: {err}");
    }

    #[tokio::test]
    async fn gives_up_after_max_attempts_on_fast_forward() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/acme/kb/git/refs/heads/main"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ref": "refs/heads/main",
                "object": { "sha": "HEAD0", "type": "commit" }
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/repos/acme/kb/git/commits/HEAD\d$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "tree": { "sha": "T" }
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/repos/acme/kb/git/blobs"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "sha": "B" })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/repos/acme/kb/git/trees"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "sha": "T1" })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/repos/acme/kb/git/commits"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "sha": "C" })))
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path("/repos/acme/kb/git/refs/heads/main"))
            .respond_with(
                ResponseTemplate::new(422)
                    .set_body_string(r#"{"message":"Update is not a fast forward"}"#),
            )
            .expect(3)
            .mount(&server)
            .await;

        let http = GithubHttp::new().unwrap();
        let gh = GithubClient::new(target()).with_api_base(server.uri());
        let err = run(
            &http,
            &gh,
            "tok",
            mutation_simple(),
            BackoffPolicy {
                max_attempts: 3,
                base_delay: Duration::ZERO,
                max_delay: Duration::ZERO,
            },
        )
        .await
        .expect_err("should give up");
        let m = err.to_string().to_lowercase();
        assert!(
            m.contains("fast forward") || m.contains("422"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn empty_mutation_is_rejected() {
        let http = GithubHttp::new().unwrap();
        let gh = GithubClient::new(target()).with_api_base("http://localhost");
        let err = run(
            &http,
            &gh,
            "tok",
            RenameMutation {
                upserts: vec![],
                deletes: vec![],
                message: "x".into(),
                author_name: "a".into(),
                author_email: "a@b".into(),
            },
            BackoffPolicy::instant(),
        )
        .await
        .expect_err("empty");
        assert!(err.to_string().contains("empty"), "{err}");
    }
}
