use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use brain_domain::TargetRef;

use super::ApiError;
#[cfg(feature = "ssr")]
use super::sfe;

/// Read-only summary of an open pull request, surfaced in the PR list view.
/// `mergeable` / check status are intentionally absent — they require a
/// per-PR fetch and belong to a later slice (merge-from-UI).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PrSummary {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub draft: bool,
    pub author: String,
    pub created_at: String,
    pub head_ref: String,
    pub base_ref: String,
}

#[server(ListOpenPrs, "/api", endpoint = "list_open_prs")]
pub async fn list_open_prs(target: TargetRef) -> Result<Vec<PrSummary>, ApiError> {
    use crate::server::session;

    let (_s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let target = super::target_from_ref(target).map_err(sfe)?;
    let storage = session::storage_for(target.clone()).map_err(sfe)?;
    let permissions = storage.repository_permissions(&token).await.map_err(sfe)?;
    if !permissions.pull {
        return Err(ApiError::PermissionDenied(format!(
            "missing read permission for {}/{}",
            target.org, target.repo
        )));
    }

    storage
        .list_open_pull_requests(&token)
        .await
        .map(|prs| {
            prs.into_iter()
                .map(|p| PrSummary {
                    number: p.number,
                    title: p.title,
                    url: p.html_url,
                    draft: p.draft,
                    author: p.author,
                    created_at: p.created_at,
                    head_ref: p.head_ref,
                    base_ref: p.base_ref,
                })
                .collect()
        })
        .map_err(sfe)
}

/// Result of a successful merge — the resulting merge commit SHA.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MergePrResult {
    pub sha: String,
}

#[server(MergePullRequest, "/api", endpoint = "merge_pull_request")]
pub async fn merge_pull_request(target: TargetRef, number: u64) -> Result<MergePrResult, ApiError> {
    use crate::server::session;

    let (s, token) = session::require_session_and_token().await.map_err(sfe)?;
    let user = session::session_user_or_fallback(&s).await;
    let target = super::target_from_ref(target).map_err(sfe)?;
    let storage = session::storage_for(target.clone()).map_err(sfe)?;
    let permissions = storage.repository_permissions(&token).await.map_err(sfe)?;
    if !permissions.push {
        return Err(ApiError::PermissionDenied(format!(
            "missing write permission to merge into {}/{}",
            target.org, target.repo
        )));
    }

    let sha = storage
        .merge_pull_request(&token, number, "squash")
        .await
        .map_err(sfe)?;
    crate::server::audit::log("pr_merged", Some(&user), &format!("#{number} -> {sha}")).await;
    Ok(MergePrResult { sha })
}
