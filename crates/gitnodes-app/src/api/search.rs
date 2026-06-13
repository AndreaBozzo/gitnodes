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

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use gitnodes_domain::TargetRef;

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

    let target = super::target_from_ref(target).map_err(sfe)?;
    let _ = session::require_target_read(&target).await.map_err(sfe)?;

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
