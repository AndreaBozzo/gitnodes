//! Loader for `.brain-config.yml` from the target repo.
//!
//! Fetches the file via the GitHub Contents API on first access (per target),
//! caches the parsed `BrainConfig` for 30s (same TTL pattern as the graph
//! cache in `brain-storage`), and falls back to `BrainConfig::default()` on
//! a missing file or parse/validation failure so the app keeps working.
//!
//! The cache is keyed by `TargetKey` so a future multi-target deployment
//! cannot leak one repo's config into another's response.

use base64::Engine;
use brain_domain::{BrainConfig, BrainError, GithubClient, TargetConfig, TargetKey};
use brain_storage::GithubHttp;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const TTL: Duration = Duration::from_secs(30);
const CONFIG_PATH: &str = ".brain-config.yml";

struct CacheEntry {
    cfg: Arc<BrainConfig>,
    stored_at: Instant,
}

fn cache() -> &'static Mutex<HashMap<TargetKey, CacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<TargetKey, CacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_get(key: &TargetKey) -> Option<Arc<BrainConfig>> {
    let guard = cache().lock().ok()?;
    let entry = guard.get(key)?;
    if entry.stored_at.elapsed() < TTL {
        Some(entry.cfg.clone())
    } else {
        None
    }
}

fn cache_store(key: &TargetKey, cfg: Arc<BrainConfig>) {
    if let Ok(mut guard) = cache().lock() {
        guard.insert(
            key.clone(),
            CacheEntry {
                cfg,
                stored_at: Instant::now(),
            },
        );
    }
}

/// Drop the cached config for a single target. Called from the manual
/// `RefreshGraph` server fn and from any future webhook push handler.
pub fn invalidate(key: &TargetKey) {
    if let Ok(mut guard) = cache().lock() {
        guard.remove(key);
    }
}

/// Seed the cache with a canonical config we just wrote. Avoids a read-after-write
/// race against GitHub's contents API, which can serve the pre-write blob for a
/// few hundred ms after `PUT /contents/{path}` returns success — long enough for
/// the post-save reload to repopulate the cache with stale data and pin it for
/// the 30s TTL.
pub fn store(key: &TargetKey, cfg: BrainConfig) {
    cache_store(key, Arc::new(cfg));
}

#[derive(Deserialize)]
struct ContentResponse {
    content: String,
}

/// Load the config from the repo, or return the default on any non-fatal
/// failure. Never returns `Err` from the caller's perspective — a malformed
/// `.brain-config.yml` must not take the whole app down.
pub async fn load(target: &TargetConfig, token: &str) -> Arc<BrainConfig> {
    let key = TargetKey::from(target);
    if let Some(hit) = cache_get(&key) {
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
    cache_store(&key, arc.clone());
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
    // The transport is target-agnostic — pull it from context for connection
    // pooling — but the URL **must** be built from the explicit `target`
    // argument so a multi-target caller (Phase 3) can load any repo's config
    // without being silently rerouted to the startup default.
    let http = match leptos::prelude::use_context::<GithubHttp>() {
        Some(h) => h,
        None => GithubHttp::new()?,
    };
    let gh = GithubClient::new(target.clone());
    let url = format!("{}?ref={}", gh.contents_url(CONFIG_PATH), target.branch);
    let resp = http
        .get(&url, token)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn target(org: &str, repo: &str, branch: &str) -> TargetConfig {
        TargetConfig {
            org: org.into(),
            repo: repo.into(),
            branch: branch.into(),
        }
    }

    #[test]
    fn cache_isolates_targets() {
        let a = TargetKey::from(&target("o", "cfg_iso_a", "main"));
        let b = TargetKey::from(&target("o", "cfg_iso_b", "main"));
        invalidate(&a);
        invalidate(&b);

        cache_store(&a, Arc::new(BrainConfig::default()));
        assert!(cache_get(&a).is_some());
        assert!(cache_get(&b).is_none());

        invalidate(&a);
        assert!(cache_get(&a).is_none());
    }
}
