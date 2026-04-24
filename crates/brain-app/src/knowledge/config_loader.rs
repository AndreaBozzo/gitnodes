//! Loader for `.brain-config.yml` from the target repo.
//!
//! Fetches the file via the GitHub Contents API on first access (per-process),
//! caches the parsed `BrainConfig` for 30s (same TTL pattern as the graph
//! cache in `brain-storage`), and falls back to `BrainConfig::default()` on
//! a missing file or parse/validation failure so the app keeps working.

use base64::Engine;
use brain_domain::{BrainConfig, BrainError, GithubClient, TargetConfig};
use serde::Deserialize;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const TTL: Duration = Duration::from_secs(30);
const CONFIG_PATH: &str = ".brain-config.yml";

struct CacheEntry {
    cfg: Arc<BrainConfig>,
    stored_at: Instant,
}

static CACHE: Mutex<Option<CacheEntry>> = Mutex::new(None);

fn cache_get() -> Option<Arc<BrainConfig>> {
    let guard = CACHE.lock().ok()?;
    let entry = guard.as_ref()?;
    if entry.stored_at.elapsed() < TTL {
        Some(entry.cfg.clone())
    } else {
        None
    }
}

fn cache_store(cfg: Arc<BrainConfig>) {
    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some(CacheEntry {
            cfg,
            stored_at: Instant::now(),
        });
    }
}

/// Clear the cache. Called from write paths that might change the config
/// (Phase 2: webhook push events).
pub fn invalidate() {
    if let Ok(mut guard) = CACHE.lock() {
        *guard = None;
    }
}

#[derive(Deserialize)]
struct ContentResponse {
    content: String,
}

/// Load the config from the repo, or return the default on any non-fatal
/// failure. Never returns `Err` from the caller's perspective — a malformed
/// `.brain-config.yml` must not take the whole app down.
pub async fn load(target: &TargetConfig, token: &str) -> Arc<BrainConfig> {
    if let Some(hit) = cache_get() {
        return hit;
    }
    let cfg = match fetch_and_parse(target, token).await {
        Ok(Some(cfg)) => cfg,
        Ok(None) => BrainConfig::default(),
        Err(e) => {
            tracing::warn!(error = %e, "brain config load failed, using default");
            BrainConfig::default()
        }
    };
    let arc = Arc::new(cfg);
    cache_store(arc.clone());
    arc
}

/// Returns `Ok(Some(cfg))` on a valid file, `Ok(None)` on 404 (file absent),
/// and `Err` on any other unrecoverable error. A malformed or
/// validation-failing YAML is reported as `Err` and the caller logs+falls
/// back to default.
async fn fetch_and_parse(
    target: &TargetConfig,
    token: &str,
) -> Result<Option<BrainConfig>, BrainError> {
    let client = reqwest::Client::builder()
        .user_agent("brain_ui")
        .build()
        .map_err(|e| BrainError::Io(format!("http client: {e}")))?;
    let gh = GithubClient::new(target.clone());
    let url = format!("{}?ref={}", gh.contents_url(CONFIG_PATH), target.branch);
    let resp = client
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| BrainError::github(format!("config fetch: {e}")))?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let resp = resp
        .error_for_status()
        .map_err(|e| BrainError::github(format!("config status: {e}")))?;
    let body: ContentResponse = resp
        .json()
        .await
        .map_err(|e| BrainError::github(format!("config parse: {e}")))?;

    let cleaned: String = body
        .content
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(cleaned)
        .map_err(|e| BrainError::parse(format!("config b64: {e}")))?;
    let text =
        String::from_utf8(bytes).map_err(|e| BrainError::parse(format!("config utf8: {e}")))?;

    let cfg = BrainConfig::parse(&text).map_err(|e| BrainError::parse(e.to_string()))?;
    Ok(Some(cfg))
}
