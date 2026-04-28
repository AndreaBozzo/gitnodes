use brain_domain::{BrainError, TargetConfig};
use brain_storage::Storage;

use super::{WriteResult, slugify};

#[allow(clippy::too_many_arguments)]
pub(super) async fn save_file_permission_aware(
    storage: &brain_storage::GithubStorage,
    token: &str,
    path: &str,
    content: &str,
    sha: Option<&str>,
    message: &str,
    user: &str,
    author_email: &str,
    target: &TargetConfig,
) -> Result<WriteResult, BrainError> {
    let permissions = storage.repository_permissions(token).await?;
    if permissions.push {
        match storage
            .save_file(token, path, content, sha, message, user, author_email)
            .await
        {
            Ok(path) => return Ok(WriteResult::direct(path)),
            Err(error) if should_fallback_to_pr(&error) => {}
            Err(error) => return Err(error),
        }
    }

    let plan =
        prepare_pr_write(storage, token, user, target, "save", path, permissions.push).await?;
    let written_path = plan
        .storage
        .save_file(token, path, content, sha, message, user, author_email)
        .await?;
    let pr = open_write_pr(
        storage,
        token,
        &plan,
        &format!("Propose {path} via Brain UI"),
        &format!("Brain UI could not write directly to `{}` and proposed this change through a pull request instead.\n\nTouched path: `{path}`", target.branch),
    )
    .await?;
    Ok(WriteResult::pull_request(
        written_path,
        plan.branch,
        pr.number,
        pr.html_url,
    ))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn delete_file_permission_aware(
    storage: &brain_storage::GithubStorage,
    token: &str,
    path: &str,
    sha: &str,
    message: &str,
    user: &str,
    author_email: &str,
    target: &TargetConfig,
) -> Result<WriteResult, BrainError> {
    let permissions = storage.repository_permissions(token).await?;
    if permissions.push {
        match storage
            .delete_file(token, path, sha, message, user, author_email)
            .await
        {
            Ok(()) => return Ok(WriteResult::direct(path)),
            Err(error) if should_fallback_to_pr(&error) => {}
            Err(error) => return Err(error),
        }
    }

    let plan = prepare_pr_write(
        storage,
        token,
        user,
        target,
        "delete",
        path,
        permissions.push,
    )
    .await?;
    plan.storage
        .delete_file(token, path, sha, message, user, author_email)
        .await?;
    let pr = open_write_pr(
        storage,
        token,
        &plan,
        &format!("Propose deleting {path} via Brain UI"),
        &format!("Brain UI could not delete `{path}` directly from `{}` and proposed the deletion through a pull request instead.", target.branch),
    )
    .await?;
    Ok(WriteResult::pull_request(
        path,
        plan.branch,
        pr.number,
        pr.html_url,
    ))
}

pub(super) fn should_fallback_to_pr(error: &BrainError) -> bool {
    match error {
        BrainError::GitHub(message) => {
            message.contains("403")
                || message.to_lowercase().contains("protected")
                || message.to_lowercase().contains("resource not accessible")
        }
        _ => false,
    }
}

pub(super) struct PrWritePlan {
    pub(super) storage: brain_storage::GithubStorage,
    pub(super) branch: String,
    head: String,
}

pub(super) async fn prepare_pr_write(
    upstream_storage: &brain_storage::GithubStorage,
    token: &str,
    user: &str,
    target: &TargetConfig,
    action: &str,
    path: &str,
    can_push_upstream: bool,
) -> Result<PrWritePlan, BrainError> {
    let base_sha = upstream_storage.head_sha(token).await?;
    let branch = pr_branch_name(user, action, path);

    if can_push_upstream {
        upstream_storage
            .create_branch_from_sha(token, &branch, &base_sha)
            .await?;
        let branch_target = TargetConfig {
            org: target.org.clone(),
            repo: target.repo.clone(),
            branch: branch.clone(),
        };
        return Ok(PrWritePlan {
            storage: brain_storage::GithubStorage::new(
                upstream_storage.http().clone(),
                branch_target,
            ),
            branch: branch.clone(),
            head: branch,
        });
    }

    upstream_storage.ensure_fork(token, user).await?;
    let fork_target = TargetConfig {
        org: user.to_string(),
        repo: target.repo.clone(),
        branch: branch.clone(),
    };
    let fork_storage =
        brain_storage::GithubStorage::new(upstream_storage.http().clone(), fork_target);
    create_branch_with_retry(&fork_storage, token, &branch, &base_sha).await?;
    Ok(PrWritePlan {
        storage: fork_storage,
        branch: branch.clone(),
        head: format!("{user}:{branch}"),
    })
}

async fn create_branch_with_retry(
    storage: &brain_storage::GithubStorage,
    token: &str,
    branch: &str,
    sha: &str,
) -> Result<(), BrainError> {
    let delays = [
        std::time::Duration::from_millis(0),
        std::time::Duration::from_millis(1_000),
        std::time::Duration::from_millis(2_000),
        std::time::Duration::from_millis(4_000),
    ];
    let mut last_error = None;
    for delay in delays {
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        match storage.create_branch_from_sha(token, branch, sha).await {
            Ok(()) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| BrainError::github("branch create failed")))
}

pub(super) async fn open_write_pr(
    upstream_storage: &brain_storage::GithubStorage,
    token: &str,
    plan: &PrWritePlan,
    title: &str,
    body: &str,
) -> Result<brain_storage::PullRequestOutcome, BrainError> {
    upstream_storage
        .open_pull_request(
            token,
            &plan.head,
            &upstream_storage.target().branch,
            title,
            body,
        )
        .await
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
    storage: &brain_storage::GithubStorage,
    target: &TargetConfig,
    token: &str,
    user: &str,
    reason: &str,
) {
    use brain_domain::TargetKey;
    let key = TargetKey::from(target);
    brain_storage::invalidate(&key);
    brain_storage::invalidate_template(&key);
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
