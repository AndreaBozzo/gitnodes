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

use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;
use brain_domain::{BrainError, Edge, Node, TargetConfig};
use brain_graph::{RawFile, build_graph, is_included_md};
use serde::Deserialize;

pub trait Storage: Send + Sync {
    async fn load_template(&self, token: &str, filename: &str) -> Result<String, BrainError>;
    async fn load_graph(&self, token: &str) -> Result<(Vec<Node>, Vec<Edge>), BrainError>;
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
    async fn list_folders(&self, token: &str) -> Result<Vec<String>, BrainError>;
    fn invalidate_cache(&self);
}

pub struct GithubStorage {
    cfg: TargetConfig,
}

impl GithubStorage {
    pub fn new(cfg: TargetConfig) -> Self {
        Self { cfg }
    }

    fn contents_url(&self, path: &str) -> String {
        self.cfg.contents_url(path)
    }

    fn branch(&self) -> &str {
        &self.cfg.branch
    }
}

const CACHE_TTL: Duration = Duration::from_secs(30);

struct CacheEntry {
    stored_at: Instant,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
}

static CACHE: Mutex<Option<CacheEntry>> = Mutex::new(None);

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

pub fn http_client() -> Result<reqwest::Client, BrainError> {
    reqwest::Client::builder()
        .user_agent("brain_ui")
        .build()
        .map_err(|e| BrainError::Io(format!("http client: {e}")))
}

impl Storage for GithubStorage {
    async fn load_template(&self, token: &str, filename: &str) -> Result<String, BrainError> {
        if let Some(hit) = template_cache_get(filename) {
            return Ok(hit);
        }
        let client = http_client()?;
        let url = format!(
            "{}?ref={}",
            self.contents_url(&format!("templates/{filename}")),
            self.branch()
        );
        let body: ContentResponse = client
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| BrainError::github(format!("template fetch: {e}")))?
            .error_for_status()
            .map_err(|e| BrainError::github(format!("template status: {e}")))?
            .json()
            .await
            .map_err(|e| BrainError::github(format!("template parse: {e}")))?;
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
        template_cache_store(filename, &text);
        Ok(text)
    }

    async fn load_graph(&self, token: &str) -> Result<(Vec<Node>, Vec<Edge>), BrainError> {
        if let Some(hit) = cache_get() {
            return Ok(hit);
        }
        let client = http_client()?;
        let tree_url = self.cfg.tree_url();
        let tree: TreeResponse = client
            .get(&tree_url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| BrainError::github(format!("tree fetch: {e}")))?
            .error_for_status()
            .map_err(|e| BrainError::github(format!("tree status: {e}")))?
            .json()
            .await
            .map_err(|e| BrainError::github(format!("tree parse: {e}")))?;

        let mut candidates: Vec<String> = tree
            .tree
            .into_iter()
            .filter(|e| e.kind == "blob")
            .filter(|e| is_included_md(&e.path))
            .map(|e| e.path)
            .collect();
        candidates.sort();

        let mut files: Vec<RawFile> = Vec::with_capacity(candidates.len());
        for path in &candidates {
            let url = format!("{}?ref={}", self.contents_url(path), self.branch());
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
        let (nodes, edges) = build_graph(&files);
        cache_store(&nodes, &edges);
        Ok((nodes, edges))
    }

    async fn read_file(&self, token: &str, path: &str) -> Result<(String, String), BrainError> {
        let client = http_client()?;
        let url = format!("{}?ref={}", self.contents_url(path), self.branch());
        let resp: ContentResponse = client
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| BrainError::github(format!("get_content: {e}")))?
            .error_for_status()
            .map_err(|e| BrainError::github(format!("content status: {e}")))?
            .json()
            .await
            .map_err(|e| BrainError::github(format!("content parse: {e}")))?;

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
        let client = http_client()?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(content.as_bytes());

        let mut body = serde_json::json!({
            "message": message,
            "content": encoded,
            "branch": self.branch(),
            "committer": {
                "name": author_name,
                "email": author_email,
            }
        });

        if let Some(s) = sha
            && !s.is_empty()
        {
            body["sha"] = serde_json::json!(s);
        }

        let url = self.contents_url(path);
        let response = client
            .put(&url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .map_err(|e| BrainError::github(format!("PUT: {e}")))?;

        if response.status().is_success() {
            invalidate();
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

    async fn delete_file(
        &self,
        token: &str,
        path: &str,
        sha: &str,
        message: &str,
        author_name: &str,
        author_email: &str,
    ) -> Result<(), BrainError> {
        let client = http_client()?;
        let body = serde_json::json!({
            "message": message,
            "sha": sha,
            "branch": self.branch(),
            "committer": {
                "name": author_name,
                "email": author_email,
            }
        });

        let url = self.contents_url(path);
        let response = client
            .delete(&url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .map_err(|e| BrainError::github(format!("DELETE: {e}")))?;

        if response.status().is_success() {
            invalidate();
            Ok(())
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

    async fn list_folders(&self, token: &str) -> Result<Vec<String>, BrainError> {
        let client = http_client()?;
        let url = format!("{}?ref={}", self.contents_url(""), self.branch());
        let items: Vec<GhDirEntry> = client
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| BrainError::github(format!("get_content: {e}")))?
            .error_for_status()
            .map_err(|e| BrainError::github(format!("content status: {e}")))?
            .json()
            .await
            .map_err(|e| BrainError::github(format!("content parse: {e}")))?;

        let folders: Vec<String> = items
            .iter()
            .filter(|item| item.kind == "dir")
            .map(|item| item.path.clone())
            .collect();

        Ok(folders)
    }

    fn invalidate_cache(&self) {
        invalidate();
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

    async fn load_graph(&self, _token: &str) -> Result<(Vec<Node>, Vec<Edge>), BrainError> {
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

    async fn list_folders(&self, _token: &str) -> Result<Vec<String>, BrainError> {
        Ok(Vec::new())
    }

    fn invalidate_cache(&self) {}
}
