//! Live graph loader: fetches the Brain repo via the GitHub API using the
//! caller's session token, then delegates to `brain-graph` to build the
//! `(Vec<Node>, Vec<Edge>)` the UI consumes.
//!
//! Parsing, graph construction, and layout are pure and live in `brain-graph`.
//! This module owns only the I/O: HTTP calls + in-memory TTL caches.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;
use brain_graph::{RawFile, build_graph, is_included_md};
use serde::Deserialize;

use super::types::{Edge, Node};

const OWNER: &str = "Dritara-Digital";
const REPO: &str = "Brain";

/// In-memory TTL cache for the full graph. The repo contents are identical for
/// every authed org member, so a process-wide cache is safe — no need to key
/// by user. Kept short (30s) so edits made outside the UI still surface quickly.
const CACHE_TTL: Duration = Duration::from_secs(30);

struct CacheEntry {
    stored_at: Instant,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
}

static CACHE: Mutex<Option<CacheEntry>> = Mutex::new(None);

/// Longer TTL for template bodies — they rarely change.
const TEMPLATE_TTL: Duration = Duration::from_secs(600);

struct TemplateEntry {
    stored_at: Instant,
    body: String,
}

static TEMPLATE_CACHE: Mutex<Option<std::collections::HashMap<String, TemplateEntry>>> =
    Mutex::new(None);

fn template_cache_get(filename: &str) -> Option<String> {
    let guard = TEMPLATE_CACHE.lock().ok()?;
    let map = guard.as_ref()?;
    let entry = map.get(filename)?;
    if entry.stored_at.elapsed() > TEMPLATE_TTL {
        return None;
    }
    Some(entry.body.clone())
}

fn template_cache_store(filename: &str, body: &str) {
    if let Ok(mut guard) = TEMPLATE_CACHE.lock() {
        let map = guard.get_or_insert_with(std::collections::HashMap::new);
        map.insert(
            filename.to_string(),
            TemplateEntry {
                stored_at: Instant::now(),
                body: body.to_string(),
            },
        );
    }
}

/// Fetch a template file from `templates/{filename}` in the Brain repo.
/// Returns the raw markdown (frontmatter + body). Cached for 10 minutes.
pub async fn load_template(token: &str, filename: &str) -> Result<String, String> {
    if let Some(hit) = template_cache_get(filename) {
        return Ok(hit);
    }
    let client = reqwest::Client::builder()
        .user_agent("brain_ui")
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let url = format!(
        "https://api.github.com/repos/{OWNER}/{REPO}/contents/templates/{filename}?ref=main"
    );
    let body: ContentResponse = client
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("template fetch: {e}"))?
        .error_for_status()
        .map_err(|e| format!("template status: {e}"))?
        .json()
        .await
        .map_err(|e| format!("template parse: {e}"))?;
    let cleaned: String = body
        .content
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(cleaned)
        .map_err(|e| format!("template b64: {e}"))?;
    let text = String::from_utf8(bytes).map_err(|e| format!("template utf8: {e}"))?;
    template_cache_store(filename, &text);
    Ok(text)
}

/// Drop any cached graph. Called after a successful write (save/delete) so the
/// next `/knowledge` render picks up the change immediately instead of waiting
/// for the TTL.
pub fn invalidate() {
    if let Ok(mut guard) = CACHE.lock() {
        *guard = None;
    }
}

fn cache_get() -> Option<(Vec<Node>, Vec<Edge>)> {
    let guard = CACHE.lock().ok()?;
    let entry = guard.as_ref()?;
    if entry.stored_at.elapsed() > CACHE_TTL {
        return None;
    }
    Some((entry.nodes.clone(), entry.edges.clone()))
}

fn cache_store(nodes: &[Node], edges: &[Edge]) {
    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some(CacheEntry {
            stored_at: Instant::now(),
            nodes: nodes.to_vec(),
            edges: edges.to_vec(),
        });
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
struct ContentResponse {
    content: String,
    sha: String,
}

pub async fn load_graph(token: &str) -> Result<(Vec<Node>, Vec<Edge>), String> {
    if let Some(hit) = cache_get() {
        return Ok(hit);
    }

    let client = reqwest::Client::builder()
        .user_agent("brain_ui")
        .build()
        .map_err(|e| format!("http client: {e}"))?;

    // 1. One recursive tree call — all paths + blob SHAs in a single request.
    let tree_url =
        format!("https://api.github.com/repos/{OWNER}/{REPO}/git/trees/main?recursive=1");
    let tree: TreeResponse = client
        .get(&tree_url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("tree fetch: {e}"))?
        .error_for_status()
        .map_err(|e| format!("tree status: {e}"))?
        .json()
        .await
        .map_err(|e| format!("tree parse: {e}"))?;

    // 2. Filter to the set of markdown files brain-graph cares about.
    let mut candidates: Vec<String> = tree
        .tree
        .into_iter()
        .filter(|e| e.kind == "blob")
        .filter(|e| is_included_md(&e.path))
        .map(|e| e.path)
        .collect();
    candidates.sort();

    // 3. Fetch each file's content. Transient failures are logged and skipped.
    let mut files: Vec<RawFile> = Vec::with_capacity(candidates.len());
    for path in &candidates {
        let url = format!("https://api.github.com/repos/{OWNER}/{REPO}/contents/{path}?ref=main");
        let resp = match client.get(&url).bearer_auth(token).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(path, error = %e, "content fetch failed");
                continue;
            }
        };
        let status = resp.status();
        if !status.is_success() {
            tracing::warn!(path, %status, "content fetch non-success");
            continue;
        }
        let body: ContentResponse = match resp.json().await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(path, error = %e, "content parse failed");
                continue;
            }
        };
        let cleaned: String = body
            .content
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let bytes = match base64::engine::general_purpose::STANDARD.decode(cleaned) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(path, error = %e, "base64 decode failed");
                continue;
            }
        };
        let text = match String::from_utf8(bytes) {
            Ok(t) => t,
            Err(_) => {
                tracing::warn!(path, "non-utf8 content, skipping");
                continue;
            }
        };
        files.push(RawFile {
            path: path.clone(),
            sha: body.sha,
            content: text,
        });
    }

    // 4. Delegate the pure parse/build/layout to brain-graph.
    let (nodes, edges) = build_graph(&files);
    cache_store(&nodes, &edges);
    Ok((nodes, edges))
}
