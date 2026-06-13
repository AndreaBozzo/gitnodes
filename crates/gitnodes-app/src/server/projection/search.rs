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

use std::collections::{HashMap, HashSet};

use gitnodes_domain::{BrainError, TargetConfig};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};

use super::{normalize_path_prefix, pool, sqlx_error, target::ensure_target_id};

const RRF_K: f64 = 60.0;
const DEFAULT_LIMIT: usize = 30;
const HARD_LIMIT: usize = 100;

#[derive(Clone, Debug, Default)]
pub struct SearchFilters {
    pub q: String,
    pub node_types: Vec<String>,
    pub tags: Vec<String>,
    pub path_prefix: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SearchHit {
    pub path: String,
    pub title: String,
    pub snippet: String,
    pub score: f64,
}

#[derive(Clone, Debug)]
struct SearchCandidate {
    node_id: i64,
    path: String,
    title: String,
    node_type: String,
    tags: Vec<String>,
    snippet: String,
    fts_rank: usize,
}

pub async fn search_nodes(
    target: &TargetConfig,
    filters: &SearchFilters,
) -> Result<Vec<SearchHit>, BrainError> {
    let pool = pool()?;
    let target_id = ensure_target_id(pool, target).await?;
    search_nodes_from_pool(pool, target_id, filters).await
}

pub(super) async fn search_nodes_from_pool(
    pool: &SqlitePool,
    target_id: i64,
    filters: &SearchFilters,
) -> Result<Vec<SearchHit>, BrainError> {
    let Some(match_query) = fts_match_query(&filters.q) else {
        return Ok(Vec::new());
    };

    let limit = filters.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, HARD_LIMIT);
    let candidate_limit = (limit * 5).min(500) as i64;
    let wanted_tags: HashSet<String> = filters.tags.iter().map(|t| t.to_lowercase()).collect();
    let path_prefix = filters
        .path_prefix
        .as_deref()
        .map(normalize_path_prefix)
        .filter(|p| !p.is_empty());

    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT
            node_id,
            path,
            title,
            node_type,
            tags,
            snippet(node_search_fts, -1, '[', ']', '...', 24) AS snippet,
            bm25(node_search_fts, 0.0, 0.0, 5.0, 0.0, 10.0, 3.0, 2.0, 1.0) AS rank
         FROM node_search_fts
         WHERE node_search_fts MATCH ",
    );
    query.push_bind(match_query);
    query.push(" AND target_id = ");
    query.push_bind(target_id);

    if !filters.node_types.is_empty() {
        query.push(" AND node_type IN (");
        let mut separated = query.separated(", ");
        for node_type in &filters.node_types {
            separated.push_bind(node_type);
        }
        separated.push_unseparated(")");
    }
    if let Some(prefix) = path_prefix.as_ref() {
        query.push(" AND path LIKE ");
        query.push_bind(format!("{prefix}%"));
    }
    query.push(" ORDER BY rank ASC, path ASC LIMIT ");
    query.push_bind(candidate_limit);

    let rows = query.build().fetch_all(pool).await.map_err(sqlx_error)?;
    let mut candidates = Vec::with_capacity(rows.len());
    for (idx, row) in rows.into_iter().enumerate() {
        let tags = tags_from_fts_row(&row)?;
        if !wanted_tags.is_empty()
            && !tags
                .iter()
                .any(|tag| wanted_tags.contains(&tag.to_lowercase()))
        {
            continue;
        }
        candidates.push(SearchCandidate {
            node_id: row.get::<i64, _>("node_id"),
            path: row.get::<String, _>("path"),
            title: row.get::<String, _>("title"),
            node_type: row.get::<String, _>("node_type"),
            tags,
            snippet: row.get::<String, _>("snippet"),
            fts_rank: idx,
        });
    }

    let fts_ranked: Vec<i64> = candidates
        .iter()
        .map(|candidate| candidate.node_id)
        .collect();
    let mut structured = candidates
        .iter()
        .filter_map(|candidate| {
            let score = structured_overlap(candidate, filters, path_prefix.as_deref());
            (score > 0).then_some((candidate.node_id, score, candidate.fts_rank))
        })
        .collect::<Vec<_>>();
    structured.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.2.cmp(&b.2)));
    let structured_ranked: Vec<i64> = structured
        .into_iter()
        .map(|(node_id, _, _)| node_id)
        .collect();
    let fused = reciprocal_rank_fusion(&fts_ranked, &structured_ranked, RRF_K);

    candidates.sort_by(|a, b| {
        let a_score = fused.get(&a.node_id).copied().unwrap_or(0.0);
        let b_score = fused.get(&b.node_id).copied().unwrap_or(0.0);
        b_score
            .total_cmp(&a_score)
            .then_with(|| a.fts_rank.cmp(&b.fts_rank))
            .then_with(|| a.path.cmp(&b.path))
    });

    Ok(candidates
        .into_iter()
        .take(limit)
        .map(|candidate| SearchHit {
            score: fused.get(&candidate.node_id).copied().unwrap_or(0.0),
            path: candidate.path,
            title: candidate.title,
            snippet: candidate.snippet,
        })
        .collect())
}

fn tags_from_fts_row(row: &sqlx::sqlite::SqliteRow) -> Result<Vec<String>, BrainError> {
    let raw = row.get::<String, _>("tags");
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str::<Vec<String>>(&raw)
        .map_err(|error| BrainError::parse(format!("search tags parse: {error}")))
}

fn structured_overlap(
    candidate: &SearchCandidate,
    filters: &SearchFilters,
    path_prefix: Option<&str>,
) -> i64 {
    let mut score = 0;
    let raw_query = filters.q.trim().to_lowercase();
    let query_tokens = search_tokens(&filters.q)
        .into_iter()
        .map(|token| token.to_lowercase())
        .collect::<Vec<_>>();
    let title = candidate.title.to_lowercase();
    let path = candidate.path.to_lowercase();
    let path_stem = path
        .rsplit('/')
        .next()
        .unwrap_or(path.as_str())
        .strip_suffix(".md")
        .unwrap_or_else(|| path.rsplit('/').next().unwrap_or(path.as_str()));

    if !raw_query.is_empty() {
        if title == raw_query {
            score += 12;
        }
        if path_stem == raw_query {
            score += 10;
        }
    }
    if !query_tokens.is_empty() {
        if query_tokens.iter().all(|token| title.contains(token)) {
            score += 6;
        }
        if query_tokens.iter().all(|token| path.contains(token)) {
            score += 5;
        }
    }
    if !filters.node_types.is_empty() && filters.node_types.contains(&candidate.node_type) {
        score += 1;
    }
    if let Some(prefix) = path_prefix
        && candidate.path.starts_with(prefix)
    {
        score += 1;
    }
    if !filters.tags.is_empty() {
        let candidate_tags: HashSet<String> = candidate
            .tags
            .iter()
            .map(|tag| tag.to_lowercase())
            .collect();
        score += filters
            .tags
            .iter()
            .filter(|tag| candidate_tags.contains(&tag.to_lowercase()))
            .count() as i64;
    }
    score
}

fn fts_match_query(raw: &str) -> Option<String> {
    let tokens = search_tokens(raw)
        .into_iter()
        .map(|token| format!("{token}*"))
        .collect::<Vec<_>>();
    (!tokens.is_empty()).then(|| tokens.join(" AND "))
}

fn search_tokens(raw: &str) -> Vec<String> {
    raw.split(|c: char| !c.is_alphanumeric() && c != '_')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

pub(super) fn reciprocal_rank_fusion<T>(primary: &[T], secondary: &[T], k: f64) -> HashMap<T, f64>
where
    T: Eq + std::hash::Hash + Clone,
{
    let mut scores = HashMap::<T, f64>::new();
    for ranked in [primary, secondary] {
        for (idx, item) in ranked.iter().enumerate() {
            *scores.entry(item.clone()).or_insert(0.0) += 1.0 / (k + idx as f64 + 1.0);
        }
    }
    scores
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts_match_query_tokenizes_user_text_safely() {
        assert_eq!(
            fts_match_query("error: E0425 borrow-checker").as_deref(),
            Some("error* AND E0425* AND borrow* AND checker*")
        );
        assert_eq!(fts_match_query("?!"), None);
    }

    #[test]
    fn rrf_fuses_two_ranked_lists() {
        let primary = vec!["a", "b", "c"];
        let secondary = vec!["c", "a"];
        let scores = reciprocal_rank_fusion(&primary, &secondary, 60.0);
        assert!(scores["a"] > scores["b"]);
        assert!(scores["c"] > scores["b"]);
    }

    #[test]
    fn rrf_preserves_primary_winner_when_secondary_ties_absent() {
        let primary = vec!["a", "b"];
        let secondary = Vec::<&str>::new();
        let scores = reciprocal_rank_fusion(&primary, &secondary, 60.0);
        assert!(scores["a"] > scores["b"]);
    }
}
