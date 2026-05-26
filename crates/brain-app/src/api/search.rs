use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use brain_domain::TargetRef;

use super::ApiError;
#[cfg(feature = "ssr")]
use super::sfe;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SearchBrainQuery {
    pub q: String,
    #[serde(default)]
    pub node_types: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SearchHit {
    pub path: String,
    pub title: String,
    pub snippet: String,
    pub score: f64,
}

#[server(
    SearchBrain,
    "/api",
    input = server_fn::codec::Json,
    endpoint = "search_brain",
)]
pub async fn search_brain(
    target: TargetRef,
    query: SearchBrainQuery,
) -> Result<Vec<SearchHit>, ApiError> {
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

    crate::server::projection::search_nodes(
        &target,
        &crate::server::projection::SearchFilters {
            q: query.q,
            node_types: query.node_types,
            tags: query.tags,
            path_prefix: query.path_prefix,
            limit: query.limit,
        },
    )
    .await
    .map(|hits| {
        hits.into_iter()
            .map(|hit| SearchHit {
                path: hit.path,
                title: hit.title,
                snippet: hit.snippet,
                score: hit.score,
            })
            .collect()
    })
    .map_err(sfe)
}
