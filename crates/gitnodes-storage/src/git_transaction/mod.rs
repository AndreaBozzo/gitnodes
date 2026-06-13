// Copyright 2026 Andrea Bozzo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Atomic Git tree mutations via the GitHub Git Data API.
//!
//! `rename_brain_file` previously issued one Contents API commit per touched
//! file (N referrers + 1 create + 1 delete = N+2 commits). This module collapses
//! those operations into a single commit by composing the lower-level Git Data
//! endpoints: `POST /git/blobs` → `POST /git/trees` → `POST /git/commits` →
//! `PATCH /git/refs/heads/{branch}`.
//!
//! Two race classes are handled distinctly:
//!
//! 1. **Fast-forward race on the ref.** A concurrent push between our
//!    `GET /git/refs/...` and our `PATCH` will return `422 Update is not a
//!    fast forward`. We retry that specific failure with exponential backoff
//!    up to a small cap. Blob SHAs are content-addressed, so on retry we
//!    reuse the blobs we already uploaded and only rebuild the tree+commit.
//!
//! 2. **Lost-update on the touched paths.** If a concurrent commit modifies
//!    one of the files we're rewriting (a referrer, the source path), simply
//!    rebasing onto the new HEAD would silently overwrite that change. The
//!    caller declares preconditions via `GitTransaction::expect_absent` and
//!    `GitTransaction::expect_sha`; before each PATCH we read the
//!    `base_tree` recursively and reject the attempt with
//!    [`BrainError::Conflict`] with a typed `ConflictKind` if any precondition no longer holds. This
//!    surfaces as "reload and retry" to the user — never as a transparent
//!    retry — because the user's edits may need to be re-derived.
//!
//! No projection mutation happens here. Callers keep their post-write
//! `rebuild_projection_after_write` step exactly as before; this preserves the
//! No Dual-Write invariant: GitHub is source of truth, the local SQLite read
//! model realigns via rebuild/sync only.

use std::collections::HashMap;
use std::time::Duration;

use base64::Engine;
use gitnodes_domain::{BrainError, ConflictKind, GithubClient};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::GithubHttp;

/// Inputs retained for the rename call site. Prefer [`GitTransaction`] for new
/// write paths; this type now maps one-to-one onto [`GitMutation`].
#[derive(Clone)]
pub struct RenameMutation {
    /// `(path, new_content)` pairs to add or update in the tree. The renamed
    /// file's *new* path must be in here; backlink-rewritten referrers also
    /// belong in this list.
    pub upserts: Vec<(String, String)>,
    /// Paths to remove from the tree. Typically `[old_path]`.
    pub deletes: Vec<String>,
    /// Paths the caller asserts are NOT yet present in the target. The most
    /// common use is the rename's destination path: a rename must not silently
    /// overwrite a file that already exists at the new location.
    pub expect_absent: Vec<String>,
    /// Paths the caller has read at a known blob sha and intends to rewrite or
    /// delete. The runtime verifies, against the exact `base_tree` we'll
    /// commit on, that each path still resolves to the expected sha. If a
    /// concurrent commit changed one of these files, the rename aborts with
    /// `BrainError::Conflict` rather than overwriting the change.
    pub expected_shas: Vec<(String, String)>,
    pub message: String,
    pub author_name: String,
    pub author_email: String,
}

/// Inputs to one Git Data API commit. Save, delete, and rename are just
/// different configurations of the same transaction:
///
/// - save: one `upsert`, plus either `expect_absent` or `expected_shas`
/// - delete: one `delete`, plus `expected_shas`
/// - rename: one or more `upsert`s, one `delete`, and both precondition kinds
#[derive(Clone)]
struct GitMutation {
    upserts: Vec<(String, Vec<u8>)>,
    deletes: Vec<String>,
    expect_absent: Vec<String>,
    expected_shas: Vec<(String, String)>,
    message: String,
    author_name: String,
    author_email: String,
}

impl From<RenameMutation> for GitMutation {
    fn from(value: RenameMutation) -> Self {
        Self {
            upserts: value
                .upserts
                .into_iter()
                .map(|(path, content)| (path, content.into_bytes()))
                .collect(),
            deletes: value.deletes,
            expect_absent: value.expect_absent,
            expected_shas: value.expected_shas,
            message: value.message,
            author_name: value.author_name,
            author_email: value.author_email,
        }
    }
}

/// Fluent builder for a Git transaction. It accumulates tree edits in memory,
/// then commits them against the current branch head with optimistic
/// preconditions and fast-forward retry.
#[derive(Clone)]
pub struct GitTransaction {
    mutation: GitMutation,
    policy: BackoffPolicy,
}

impl GitTransaction {
    pub fn new(
        message: impl Into<String>,
        author_name: impl Into<String>,
        author_email: impl Into<String>,
    ) -> Self {
        Self {
            mutation: GitMutation {
                upserts: Vec::new(),
                deletes: Vec::new(),
                expect_absent: Vec::new(),
                expected_shas: Vec::new(),
                message: message.into(),
                author_name: author_name.into(),
                author_email: author_email.into(),
            },
            policy: BackoffPolicy::default(),
        }
    }

    pub fn upsert_text(mut self, path: impl Into<String>, content: impl Into<String>) -> Self {
        self.mutation
            .upserts
            .push((path.into(), content.into().into_bytes()));
        self
    }

    pub fn upsert_bytes(mut self, path: impl Into<String>, content: impl Into<Vec<u8>>) -> Self {
        self.mutation.upserts.push((path.into(), content.into()));
        self
    }

    pub fn delete(mut self, path: impl Into<String>) -> Self {
        self.mutation.deletes.push(path.into());
        self
    }

    pub fn expect_absent(mut self, path: impl Into<String>) -> Self {
        self.mutation.expect_absent.push(path.into());
        self
    }

    pub fn expect_sha(mut self, path: impl Into<String>, sha: impl Into<String>) -> Self {
        self.mutation.expected_shas.push((path.into(), sha.into()));
        self
    }

    pub fn with_policy(mut self, policy: BackoffPolicy) -> Self {
        self.policy = policy;
        self
    }

    pub async fn commit(
        self,
        http: &GithubHttp,
        gh: &GithubClient,
        token: &str,
    ) -> Result<GitTransactionOutcome, BrainError> {
        run_transaction(http, gh, token, self.mutation, self.policy).await
    }

    /// Inspect the current branch head and evaluate optimistic preconditions
    /// without creating blobs, trees, commits, or updating a ref.
    pub async fn plan(
        &self,
        http: &GithubHttp,
        gh: &GithubClient,
        token: &str,
    ) -> Result<TransactionPlan, BrainError> {
        plan_transaction(http, gh, token, &self.mutation).await
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransactionUpsert {
    pub path: String,
    pub byte_len: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreconditionExpectation {
    Absent,
    BlobSha(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreconditionStatus {
    Satisfied,
    Failed { kind: ConflictKind, message: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransactionPrecondition {
    pub path: String,
    pub expectation: PreconditionExpectation,
    pub status: PreconditionStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransactionPlan {
    pub head_sha: String,
    pub base_tree_sha: String,
    pub upserts: Vec<TransactionUpsert>,
    pub deletes: Vec<String>,
    pub preconditions: Vec<TransactionPrecondition>,
}

impl TransactionPlan {
    pub fn can_commit(&self) -> bool {
        self.preconditions
            .iter()
            .all(|check| check.status == PreconditionStatus::Satisfied)
    }
}

/// Observable result of a successful transaction. The caller doesn't need this
/// for correctness today, but tests and future audit logging benefit from it.
#[derive(Clone, Debug)]
pub struct GitTransactionOutcome {
    pub commit_sha: String,
    pub head_before: String,
    pub head_after: String,
    pub attempts: u32,
}

/// A sequence of commits owned by one ephemeral branch lifecycle.
pub struct BranchTransaction {
    base_sha: String,
    branch: String,
    transactions: Vec<GitTransaction>,
    branch_policy: BackoffPolicy,
}

impl BranchTransaction {
    pub fn new(base_sha: impl Into<String>, branch: impl Into<String>) -> Self {
        Self {
            base_sha: base_sha.into(),
            branch: branch.into(),
            transactions: Vec::new(),
            branch_policy: BackoffPolicy {
                max_attempts: 4,
                base_delay: Duration::from_secs(1),
                max_delay: Duration::from_secs(4),
            },
        }
    }

    #[allow(clippy::should_implement_trait)] // Fluent transaction builder, not arithmetic addition.
    pub fn add(mut self, transaction: GitTransaction) -> Self {
        self.transactions.push(transaction);
        self
    }

    pub fn with_branch_policy(mut self, policy: BackoffPolicy) -> Self {
        self.branch_policy = policy;
        self
    }

    pub async fn commit_all(
        self,
        http: &GithubHttp,
        gh: &GithubClient,
        token: &str,
    ) -> Result<BranchTransactionOutcome, BrainError> {
        if self.transactions.is_empty() {
            return Err(BrainError::other("branch_transaction: no transactions"));
        }

        let branch_gh = gh.with_branch(self.branch.clone());
        create_branch_with_retry(http, &branch_gh, token, &self.base_sha, self.branch_policy)
            .await?;

        let mut commits = Vec::with_capacity(self.transactions.len());
        for transaction in self.transactions {
            match transaction.commit(http, &branch_gh, token).await {
                Ok(outcome) => commits.push(outcome),
                Err(error) => {
                    if let Err(cleanup_error) = delete_branch(http, &branch_gh, token).await {
                        tracing::warn!(
                            branch = %self.branch,
                            error = %cleanup_error,
                            "failed to roll back branch after transaction failure"
                        );
                    }
                    return Err(error);
                }
            }
        }

        let head_sha = commits
            .last()
            .map(|outcome| outcome.head_after.clone())
            .unwrap_or_else(|| self.base_sha.clone());
        Ok(BranchTransactionOutcome {
            branch: self.branch,
            head_sha,
            commits,
        })
    }
}

#[derive(Clone, Debug)]
pub struct BranchTransactionOutcome {
    pub branch: String,
    pub head_sha: String,
    pub commits: Vec<GitTransactionOutcome>,
}

impl BranchTransactionOutcome {
    pub async fn rollback(
        &self,
        http: &GithubHttp,
        gh: &GithubClient,
        token: &str,
    ) -> Result<(), BrainError> {
        delete_branch(http, &gh.with_branch(self.branch.clone()), token).await
    }
}

/// Backwards-compatible rename result.
#[derive(Debug)]
pub struct RenameOutcome {
    pub commit_sha: String,
    pub head_before: String,
    pub head_after: String,
    pub attempts: u32,
}

impl From<GitTransactionOutcome> for RenameOutcome {
    fn from(value: GitTransactionOutcome) -> Self {
        Self {
            commit_sha: value.commit_sha,
            head_before: value.head_before,
            head_after: value.head_after,
            attempts: value.attempts,
        }
    }
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
    run_transaction(http, gh, token, mutation.into(), policy)
        .await
        .map(Into::into)
}

/// Run a generic Git transaction. Reuses already-uploaded blob SHAs across
/// retries — blobs are content-addressed so a retry only redoes tree+commit+ref.
async fn run_transaction(
    http: &GithubHttp,
    gh: &GithubClient,
    token: &str,
    mutation: GitMutation,
    policy: BackoffPolicy,
) -> Result<GitTransactionOutcome, BrainError> {
    if mutation.upserts.is_empty() && mutation.deletes.is_empty() {
        return Err(BrainError::other("git_transaction: empty mutation"));
    }

    // Step 1: upload every blob exactly once. Content is fixed across retries,
    // so the resulting SHAs are stable; we cache by content to dedupe identical
    // upserts (rare but cheap to handle).
    let mut blob_shas: HashMap<Vec<u8>, String> = HashMap::new();
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
                last_error = Some(BrainError::conflict(ConflictKind::RefNonFastForward, msg));
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

async fn plan_transaction(
    http: &GithubHttp,
    gh: &GithubClient,
    token: &str,
    mutation: &GitMutation,
) -> Result<TransactionPlan, BrainError> {
    if mutation.upserts.is_empty() && mutation.deletes.is_empty() {
        return Err(BrainError::other("git_transaction: empty mutation"));
    }

    let head_sha = get_head_sha(http, gh, token).await?;
    let base_tree_sha = get_commit_tree(http, gh, token, &head_sha).await?;
    let preconditions = evaluate_preconditions(http, gh, token, &base_tree_sha, mutation).await?;

    Ok(TransactionPlan {
        head_sha,
        base_tree_sha,
        upserts: mutation
            .upserts
            .iter()
            .map(|(path, content)| TransactionUpsert {
                path: path.clone(),
                byte_len: content.len(),
            })
            .collect(),
        deletes: mutation.deletes.clone(),
        preconditions,
    })
}

async fn evaluate_preconditions(
    http: &GithubHttp,
    gh: &GithubClient,
    token: &str,
    base_tree: &str,
    mutation: &GitMutation,
) -> Result<Vec<TransactionPrecondition>, BrainError> {
    if mutation.expect_absent.is_empty() && mutation.expected_shas.is_empty() {
        return Ok(Vec::new());
    }

    let entries = get_tree_recursive(http, gh, token, base_tree).await?;
    let by_path: HashMap<&str, &str> = entries
        .iter()
        .filter(|entry| entry.kind == "blob")
        .map(|entry| (entry.path.as_str(), entry.sha.as_str()))
        .collect();
    let mut checks =
        Vec::with_capacity(mutation.expect_absent.len() + mutation.expected_shas.len());

    for path in &mutation.expect_absent {
        let status = if by_path.contains_key(path.as_str()) {
            PreconditionStatus::Failed {
                kind: ConflictKind::PathTaken,
                message: format!("destination already exists: {path}"),
            }
        } else {
            PreconditionStatus::Satisfied
        };
        checks.push(TransactionPrecondition {
            path: path.clone(),
            expectation: PreconditionExpectation::Absent,
            status,
        });
    }

    for (path, expected) in &mutation.expected_shas {
        let status = match by_path.get(path.as_str()) {
            Some(actual) if *actual == expected.as_str() => PreconditionStatus::Satisfied,
            Some(actual) => PreconditionStatus::Failed {
                kind: ConflictKind::BlobShaMoved,
                message: format!("{path} changed since read (expected {expected}, found {actual})"),
            },
            None => PreconditionStatus::Failed {
                kind: ConflictKind::RemotePathDeletedUnderUs,
                message: format!("{path} no longer exists"),
            },
        };
        checks.push(TransactionPrecondition {
            path: path.clone(),
            expectation: PreconditionExpectation::BlobSha(expected.clone()),
            status,
        });
    }

    Ok(checks)
}

fn ensure_preconditions(checks: &[TransactionPrecondition]) -> Result<(), BrainError> {
    match checks.iter().find_map(|check| match &check.status {
        PreconditionStatus::Satisfied => None,
        PreconditionStatus::Failed { kind, message } => Some((*kind, message.clone())),
    }) {
        Some((kind, message)) => Err(BrainError::conflict(kind, message)),
        None => Ok(()),
    }
}

enum AttemptError {
    /// Final ref update was rejected with `422 not a fast forward`. Safe to
    /// retry: re-read HEAD, re-check preconditions, rebuild tree+commit,
    /// re-push the ref.
    FastForward(String),
    /// Anything else (precondition failure, network error, malformed
    /// response): surface immediately, do not retry. Precondition failures
    /// in particular must reach the user as a `BrainError::Conflict` so the
    /// UI can prompt for a reload-and-retry.
    Fatal(BrainError),
}

async fn attempt_commit(
    http: &GithubHttp,
    gh: &GithubClient,
    token: &str,
    path_to_blob: &[(String, String)],
    mutation: &GitMutation,
) -> Result<GitTransactionOutcome, AttemptError> {
    let head_before = get_head_sha(http, gh, token)
        .await
        .map_err(AttemptError::Fatal)?;
    let base_tree = get_commit_tree(http, gh, token, &head_before)
        .await
        .map_err(AttemptError::Fatal)?;

    // Verify against the exact tree used as this commit's parent. The same
    // evaluator powers dry-run plans, preventing preview/commit drift.
    let preconditions = evaluate_preconditions(http, gh, token, &base_tree, mutation)
        .await
        .map_err(AttemptError::Fatal)?;
    ensure_preconditions(&preconditions).map_err(AttemptError::Fatal)?;

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
        Ok(()) => Ok(GitTransactionOutcome {
            head_before,
            head_after: commit_sha.clone(),
            commit_sha,
            attempts: 0,
        }),
        Err(e) => match classify_ref_error(&e) {
            Some(msg) => Err(AttemptError::FastForward(msg)),
            None => Err(AttemptError::Fatal(e)),
        },
    }
}

async fn sleep_with_backoff(policy: &BackoffPolicy, attempt: u32) {
    use std::sync::atomic::{AtomicU64, Ordering};

    if policy.base_delay.is_zero() {
        return;
    }
    let factor = 1u32 << (attempt - 1).min(16);
    let raw = policy.base_delay.saturating_mul(factor);
    let capped = raw.min(policy.max_delay);

    // Add up to ~40ms of jitter so concurrent retriers don't reconverge on the
    // same backoff schedule and hammer the ref-update endpoint in lockstep. A
    // process-global counter feeds a splitmix64 finalizer, so each call gets a
    // distinct, well-spread offset — unlike `Instant::now().elapsed()`, which on
    // a freshly-taken instant is a few near-constant nanoseconds. No RNG
    // dependency and no clock-entropy assumptions, so it stays wasm-safe.
    static JITTER: AtomicU64 = AtomicU64::new(0);
    let mut z = JITTER.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    let jittered = capped + Duration::from_millis(z % 41);

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
struct RecursiveTreeResponse {
    tree: Vec<RecursiveTreeEntry>,
    #[serde(default)]
    truncated: bool,
}

#[derive(Deserialize)]
struct RecursiveTreeEntry {
    path: String,
    #[serde(rename = "type")]
    kind: String,
    sha: String,
}

/// Read every blob entry under a tree SHA, resolved recursively. Used to
/// verify rename preconditions (`expect_absent`, `expected_shas`) against the
/// exact tree we're about to commit on top of.
async fn get_tree_recursive(
    http: &GithubHttp,
    gh: &GithubClient,
    token: &str,
    tree_sha: &str,
) -> Result<Vec<RecursiveTreeEntry>, BrainError> {
    let url = gh.git_tree_by_sha_url(tree_sha);
    let resp: RecursiveTreeResponse =
        GithubHttp::send_json(http.get(&url, token), "git_tree_recursive").await?;
    if resp.truncated {
        // GitHub truncates recursive tree reads above ~7MB or 100k entries.
        // Falling back here would mean implementing the paginated walk, which
        // we don't need until a real Brain repo grows that large. Until then,
        // refuse loudly so the failure is visible instead of silently
        // skipping precondition checks.
        return Err(BrainError::other(
            "git_tree_recursive: response was truncated; precondition check is unsafe",
        ));
    }
    Ok(resp.tree)
}

#[derive(Deserialize)]
struct BlobResponse {
    sha: String,
}

async fn create_blob(
    http: &GithubHttp,
    gh: &GithubClient,
    token: &str,
    content: &[u8],
) -> Result<String, BrainError> {
    let url = gh.git_blobs_url();
    let body = json!({
        "content": base64::engine::general_purpose::STANDARD.encode(content),
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

async fn create_branch_with_retry(
    http: &GithubHttp,
    gh: &GithubClient,
    token: &str,
    base_sha: &str,
    policy: BackoffPolicy,
) -> Result<(), BrainError> {
    let mut last_error = None;
    for attempt in 1..=policy.max_attempts {
        match create_branch(http, gh, token, base_sha).await {
            Ok(()) => return Ok(()),
            Err(error) if branch_creation_is_transient(&error) => {
                last_error = Some(error);
                if attempt < policy.max_attempts {
                    sleep_with_backoff(&policy, attempt).await;
                }
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_error.unwrap_or_else(|| BrainError::github("git_ref_create failed")))
}

async fn create_branch(
    http: &GithubHttp,
    gh: &GithubClient,
    token: &str,
    base_sha: &str,
) -> Result<(), BrainError> {
    let url = gh.git_refs_url();
    let body = json!({
        "ref": format!("refs/heads/{}", gh.target().branch),
        "sha": base_sha,
    });
    let resp = http
        .post(&url, token)
        .json(&body)
        .send()
        .await
        .map_err(|error| BrainError::github(format!("git_ref_create fetch: {error}")))?;
    response_without_body(resp, "git_ref_create").await
}

async fn delete_branch(
    http: &GithubHttp,
    gh: &GithubClient,
    token: &str,
) -> Result<(), BrainError> {
    let resp = http
        .delete(&gh.git_ref_url(), token)
        .send()
        .await
        .map_err(|error| BrainError::github(format!("git_ref_delete fetch: {error}")))?;
    response_without_body(resp, "git_ref_delete").await
}

async fn response_without_body(resp: reqwest::Response, context: &str) -> Result<(), BrainError> {
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    let body = resp.text().await.unwrap_or_default();
    Err(BrainError::github(format!(
        "{context} status {status}: {}",
        body.chars().take(512).collect::<String>()
    )))
}

fn branch_creation_is_transient(error: &BrainError) -> bool {
    let message = error.to_string().to_lowercase();
    if message.contains("already exists")
        || message.contains("status 401")
        || message.contains("status 403")
    {
        return false;
    }
    message.contains("status 404")
        || message.contains("status 422")
        || message.contains("status 500")
        || message.contains("status 502")
        || message.contains("status 503")
        || message.contains("status 504")
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
    use gitnodes_domain::TargetConfig;
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
            expect_absent: vec![],
            expected_shas: vec![],
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
            .and(header("user-agent", "gitnodes"))
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

    /// Mounts a recursive-tree response listing the given (path, sha) blobs.
    /// Use to drive the precondition checks deterministically.
    async fn mount_tree_recursive(server: &MockServer, tree_sha: &str, blobs: &[(&str, &str)]) {
        let entries: Vec<serde_json::Value> = blobs
            .iter()
            .map(|(p, s)| json!({ "path": p, "type": "blob", "sha": s, "mode": "100644" }))
            .collect();
        Mock::given(method("GET"))
            .and(path(format!("/repos/acme/kb/git/trees/{tree_sha}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "sha": tree_sha,
                "tree": entries,
                "truncated": false,
            })))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn outcome_head_after_is_commit_sha_not_tree_sha() {
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
        .expect("ok");
        assert_eq!(
            outcome.head_after, "COMMIT1",
            "head_after must be the new commit sha, not the tree sha"
        );
        assert_ne!(outcome.head_after, "TREE1");
    }

    #[tokio::test]
    async fn destination_collision_aborts_with_conflict() {
        let server = MockServer::start().await;
        ok_pipeline(&server).await;
        mount_patch_ok(&server).await;
        // Base tree already contains the destination path: rename must abort
        // before creating the new tree.
        mount_tree_recursive(
            &server,
            "TREE0",
            &[
                ("notes/new.md", "EXISTING_BLOB"),
                ("notes/old.md", "OLD_BLOB"),
            ],
        )
        .await;

        let http = GithubHttp::new().unwrap();
        let gh = GithubClient::new(target()).with_api_base(server.uri());
        let mut m = mutation_simple();
        m.expect_absent = vec!["notes/new.md".into()];

        let err = run(&http, &gh, "tok", m, BackoffPolicy::instant())
            .await
            .expect_err("destination occupied");
        assert!(
            matches!(
                err,
                BrainError::Conflict {
                    kind: ConflictKind::PathTaken,
                    ..
                }
            ),
            "expected PathTaken conflict, got {err:?}"
        );
        assert!(err.to_string().contains("notes/new.md"), "{err}");
    }

    #[tokio::test]
    async fn referrer_drift_aborts_with_conflict() {
        let server = MockServer::start().await;
        ok_pipeline(&server).await;
        mount_patch_ok(&server).await;
        // Caller expected referrer at OLD_REF_BLOB, but the live base_tree has
        // it at NEW_REF_BLOB — a concurrent commit changed it. Rename must
        // abort instead of overwriting that change.
        mount_tree_recursive(
            &server,
            "TREE0",
            &[
                ("notes/old.md", "OLD_BLOB"),
                ("notes/refs.md", "NEW_REF_BLOB"),
            ],
        )
        .await;

        let http = GithubHttp::new().unwrap();
        let gh = GithubClient::new(target()).with_api_base(server.uri());
        let mut m = mutation_simple();
        m.expected_shas = vec![("notes/refs.md".into(), "OLD_REF_BLOB".into())];

        let err = run(&http, &gh, "tok", m, BackoffPolicy::instant())
            .await
            .expect_err("drift detected");
        assert!(
            matches!(
                err,
                BrainError::Conflict {
                    kind: ConflictKind::BlobShaMoved,
                    ..
                }
            ),
            "expected BlobShaMoved conflict, got {err:?}"
        );
        assert!(err.to_string().contains("notes/refs.md"), "{err}");
    }

    #[tokio::test]
    async fn missing_expected_path_aborts_with_conflict() {
        let server = MockServer::start().await;
        ok_pipeline(&server).await;
        mount_patch_ok(&server).await;
        // The base_tree no longer contains old_path at all (someone deleted
        // it concurrently). expected_shas must catch this.
        mount_tree_recursive(&server, "TREE0", &[("unrelated.md", "X")]).await;

        let http = GithubHttp::new().unwrap();
        let gh = GithubClient::new(target()).with_api_base(server.uri());
        let mut m = mutation_simple();
        m.expected_shas = vec![("notes/old.md".into(), "OLD_BLOB".into())];

        let err = run(&http, &gh, "tok", m, BackoffPolicy::instant())
            .await
            .expect_err("missing path");
        assert!(
            matches!(
                err,
                BrainError::Conflict {
                    kind: ConflictKind::RemotePathDeletedUnderUs,
                    ..
                }
            ),
            "got {err:?}"
        );
        assert!(err.to_string().contains("no longer exists"), "{err}");
    }

    #[tokio::test]
    async fn precondition_rechecked_against_new_base_tree_on_retry() {
        // First attempt: PATCH 422 fast-forward. HEAD jumps to HEAD1 with a
        // *different* base_tree (TREE_AFTER) where the destination has
        // appeared. The retry must re-read the recursive tree against
        // TREE_AFTER and fail with Conflict instead of silently overwriting.
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
            .and(path("/repos/acme/kb/git/commits/HEAD0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "tree": { "sha": "TREE_BEFORE" }
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repos/acme/kb/git/commits/HEAD1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "tree": { "sha": "TREE_AFTER" }
            })))
            .mount(&server)
            .await;
        // Before the race: destination is absent.
        mount_tree_recursive(&server, "TREE_BEFORE", &[("notes/old.md", "OLD_BLOB")]).await;
        // After the race: destination has appeared.
        mount_tree_recursive(
            &server,
            "TREE_AFTER",
            &[("notes/old.md", "OLD_BLOB"), ("notes/new.md", "INTRUDER")],
        )
        .await;
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
        // First PATCH: 422 fast-forward → triggers retry.
        Mock::given(method("PATCH"))
            .and(path("/repos/acme/kb/git/refs/heads/main"))
            .respond_with(
                ResponseTemplate::new(422)
                    .set_body_string(r#"{"message":"Update is not a fast forward"}"#),
            )
            .up_to_n_times(1)
            .mount(&server)
            .await;
        // Second PATCH would 200 — but precondition check on TREE_AFTER must
        // fail first, so the second PATCH should never fire.
        Mock::given(method("PATCH"))
            .and(path("/repos/acme/kb/git/refs/heads/main"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "object": { "sha": "COMMIT1" }
            })))
            .expect(0)
            .mount(&server)
            .await;

        let http = GithubHttp::new().unwrap();
        let gh = GithubClient::new(target()).with_api_base(server.uri());
        let mut m = mutation_simple();
        m.expect_absent = vec!["notes/new.md".into()];

        let err = run(&http, &gh, "tok", m, BackoffPolicy::instant())
            .await
            .expect_err("retry must surface conflict");
        assert!(
            matches!(
                err,
                BrainError::Conflict {
                    kind: ConflictKind::PathTaken,
                    ..
                }
            ),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn truncated_tree_response_is_rejected() {
        let server = MockServer::start().await;
        ok_pipeline(&server).await;
        mount_patch_ok(&server).await;
        Mock::given(method("GET"))
            .and(path("/repos/acme/kb/git/trees/TREE0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "sha": "TREE0",
                "tree": [],
                "truncated": true,
            })))
            .mount(&server)
            .await;

        let http = GithubHttp::new().unwrap();
        let gh = GithubClient::new(target()).with_api_base(server.uri());
        let mut m = mutation_simple();
        m.expect_absent = vec!["notes/new.md".into()];

        let err = run(&http, &gh, "tok", m, BackoffPolicy::instant())
            .await
            .expect_err("truncated tree");
        assert!(err.to_string().contains("truncated"), "{err}");
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
                expect_absent: vec![],
                expected_shas: vec![],
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
