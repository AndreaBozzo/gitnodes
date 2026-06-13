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

use gitnodes_domain::{BrainError, TargetConfig, WriteIntent};
use gitnodes_storage::{BranchTransaction, GitTransaction};

use super::{WriteResult, slugify};

#[allow(clippy::too_many_arguments)]
pub(super) async fn save_file_permission_aware(
    storage: &gitnodes_storage::GithubStorage,
    token: &str,
    path: &str,
    content: &str,
    sha: Option<&str>,
    message: &str,
    user: &str,
    author_email: &str,
    target: &TargetConfig,
    intent: WriteIntent,
) -> Result<WriteResult, BrainError> {
    let permissions = crate::server::access::repository_permissions(storage, token).await?;
    let mut transaction =
        GitTransaction::new(message, user, author_email).upsert_text(path, content);
    transaction = match sha.filter(|sha| !sha.is_empty()) {
        Some(sha) => transaction.expect_sha(path, sha),
        None => transaction.expect_absent(path),
    };

    if permissions.push && intent != WriteIntent::ProposeViaPr {
        match storage.commit_transaction(token, transaction.clone()).await {
            Ok(_) => return Ok(WriteResult::direct(path)),
            Err(error) if should_fallback_to_pr(&error) => {}
            Err(error) => return Err(error),
        }
    }

    propose_transaction(
        storage,
        token,
        user,
        target,
        "save",
        path,
        permissions.push,
        transaction,
        &format!("Propose {path} via Brain UI"),
        &format!(
            "Brain UI could not write directly to `{}` and proposed this change through a pull request instead.\n\nTouched path: `{path}`",
            target.branch
        ),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn delete_file_permission_aware(
    storage: &gitnodes_storage::GithubStorage,
    token: &str,
    path: &str,
    sha: &str,
    message: &str,
    user: &str,
    author_email: &str,
    target: &TargetConfig,
) -> Result<WriteResult, BrainError> {
    let permissions = crate::server::access::repository_permissions(storage, token).await?;
    let transaction = GitTransaction::new(message, user, author_email)
        .delete(path)
        .expect_sha(path, sha);

    if permissions.push {
        match storage.commit_transaction(token, transaction.clone()).await {
            Ok(_) => return Ok(WriteResult::direct(path)),
            Err(error) if should_fallback_to_pr(&error) => {}
            Err(error) => return Err(error),
        }
    }

    propose_transaction(
        storage,
        token,
        user,
        target,
        "delete",
        path,
        permissions.push,
        transaction,
        &format!("Propose deleting {path} via Brain UI"),
        &format!(
            "Brain UI could not delete `{path}` directly from `{}` and proposed the deletion through a pull request instead.",
            target.branch
        ),
    )
    .await
}

pub(super) fn should_fallback_to_pr(error: &BrainError) -> bool {
    match error {
        BrainError::PermissionDenied(_) => true,
        BrainError::GitHub(message) => {
            message.contains("403")
                || message.to_lowercase().contains("protected")
                || message.to_lowercase().contains("resource not accessible")
        }
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn propose_transaction(
    upstream_storage: &gitnodes_storage::GithubStorage,
    token: &str,
    user: &str,
    target: &TargetConfig,
    action: &str,
    path: &str,
    can_push_upstream: bool,
    transaction: GitTransaction,
    title: &str,
    body: &str,
) -> Result<WriteResult, BrainError> {
    let base_sha = upstream_storage.head_sha(token).await?;
    let branch = pr_branch_name(user, action, path);
    let (branch_storage, head) = if can_push_upstream {
        (
            gitnodes_storage::GithubStorage::new(upstream_storage.http().clone(), target.clone()),
            branch.clone(),
        )
    } else {
        upstream_storage.ensure_fork(token, user).await?;
        (
            gitnodes_storage::GithubStorage::new(
                upstream_storage.http().clone(),
                TargetConfig {
                    org: user.to_string(),
                    repo: target.repo.clone(),
                    branch: target.branch.clone(),
                },
            ),
            format!("{user}:{branch}"),
        )
    };

    let outcome = branch_storage
        .commit_branch_transaction(
            token,
            BranchTransaction::new(base_sha, branch.clone()).add(transaction),
        )
        .await?;

    match upstream_storage
        .open_pull_request(token, &head, &target.branch, title, body)
        .await
    {
        Ok(pr) => Ok(WriteResult::pull_request(
            path,
            branch,
            pr.number,
            pr.html_url,
        )),
        Err(error) => {
            if let Err(cleanup_error) = branch_storage
                .rollback_branch_transaction(token, &outcome)
                .await
            {
                tracing::warn!(
                    branch = %outcome.branch,
                    error = %cleanup_error,
                    "failed to roll back branch after pull request creation failure"
                );
            }
            Err(error)
        }
    }
}

fn pr_branch_name(user: &str, action: &str, path: &str) -> String {
    let ts = time::OffsetDateTime::now_utc().unix_timestamp();
    let user = slugify(user);
    let path = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".md");
    let path = slugify(path);
    format!("patch/{user}/{ts}-{action}-{path}")
}

pub(super) async fn rebuild_projection_after_write(
    storage: &gitnodes_storage::GithubStorage,
    target: &TargetConfig,
    token: &str,
    user: &str,
    reason: &str,
) {
    use gitnodes_domain::TargetKey;
    let key = TargetKey::from(target);
    gitnodes_storage::invalidate(&key);
    gitnodes_storage::invalidate_template(&key);
    crate::knowledge::config_loader::invalidate(&key);
    let config = crate::knowledge::config_loader::load(target, token).await;
    if let Err(error) = crate::server::projection::rebuild(storage, token, &config, reason).await {
        crate::server::audit::log(
            "projection_error",
            Some(user),
            &format!("{reason}: {error}"),
        )
        .await;
    }
}
