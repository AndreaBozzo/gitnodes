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

//! Loader for `.gitnodes.yml` from the target repo (legacy `.brain-config.yml`
//! is still read as a fallback for repos predating the rename).
//!
//! Fetches the file via the GitHub Contents API on first access (per target),
//! caches the parsed `BrainConfig` for 30s (same TTL pattern as the graph
//! cache in `gitnodes-storage`), and falls back to `BrainConfig::default()` on
//! a missing file or parse/validation failure so the app keeps working.
//!
//! The cache is keyed by `TargetKey` so a future multi-target deployment
//! cannot leak one repo's config into another's response.

use base64::Engine;
use gitnodes_domain::{BrainConfig, BrainError, GithubClient, TargetConfig, TargetKey};
use gitnodes_storage::GithubHttp;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const TTL: Duration = Duration::from_secs(30);
const CONFIG_PATH: &str = ".gitnodes.yml";
/// Pre-rename filename, still honoured so brains created before the GitNodes
/// rename keep loading without an immediate migration.
const LEGACY_CONFIG_PATH: &str = ".brain-config.yml";

/// Grace window during which a freshly-seeded entry is protected from
/// `invalidate()`. Sized to outlast GitHub's contents-API eventual-consistency
/// window (typically <1s) plus the time it takes a webhook push for the same
/// commit to round-trip and trigger a rebuild. If a webhook lands inside this
/// window, the seeded canonical config wins; outside it, normal invalidation
/// applies (manual refresh / out-of-band push).
const SEED_GRACE: Duration = Duration::from_secs(5);

struct CacheEntry {
    cfg: Arc<BrainConfig>,
    diagnostic: Option<ConfigLoadDiagnostic>,
    stored_at: Instant,
    /// True when this entry was placed by `store()` (post-save canonical seed).
    /// Cleared when the entry is replaced by a normal `cache_store` from `load`.
    seeded: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigLoadDiagnostic {
    pub message: String,
}

#[derive(Clone)]
pub struct ConfigLoadSnapshot {
    pub config: Arc<BrainConfig>,
    pub diagnostic: Option<ConfigLoadDiagnostic>,
}

fn cache() -> &'static Mutex<HashMap<TargetKey, CacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<TargetKey, CacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_get(key: &TargetKey) -> Option<ConfigLoadSnapshot> {
    let guard = cache().lock().ok()?;
    let entry = guard.get(key)?;
    if entry.stored_at.elapsed() < TTL {
        Some(ConfigLoadSnapshot {
            config: entry.cfg.clone(),
            diagnostic: entry.diagnostic.clone(),
        })
    } else {
        None
    }
}

fn cache_store(
    key: &TargetKey,
    cfg: Arc<BrainConfig>,
    diagnostic: Option<ConfigLoadDiagnostic>,
    seeded: bool,
) {
    if let Ok(mut guard) = cache().lock() {
        guard.insert(
            key.clone(),
            CacheEntry {
                cfg,
                diagnostic,
                stored_at: Instant::now(),
                seeded,
            },
        );
    }
}

/// Drop the cached config for a single target. Called from the manual
/// `RefreshGraph` server fn and from webhook push handlers.
///
/// Honours the post-save grace window: if a `store()` seeded the cache within
/// the last `SEED_GRACE`, the entry is preserved. This blocks the webhook
/// triggered by our own commit from racing GitHub's eventually-consistent
/// contents API and pinning a stale read.
pub fn invalidate(key: &TargetKey) {
    if let Ok(mut guard) = cache().lock() {
        if let Some(entry) = guard.get(key)
            && entry.seeded
            && entry.stored_at.elapsed() < SEED_GRACE
        {
            return;
        }
        guard.remove(key);
    }
}

/// Seed the cache with a canonical config we just wrote. Avoids a read-after-write
/// race against GitHub's contents API, which can serve the pre-write blob for a
/// few hundred ms after `PUT /contents/{path}` returns success — long enough for
/// the post-save reload to repopulate the cache with stale data and pin it for
/// the 30s TTL.
///
/// Entries placed via this function are protected from `invalidate()` for
/// `SEED_GRACE`, so the webhook fired by our own commit cannot race in and
/// overwrite the seed with a stale GitHub read.
pub fn store(key: &TargetKey, cfg: BrainConfig) {
    cache_store(key, Arc::new(cfg), None, true);
}

#[derive(Deserialize)]
struct ContentResponse {
    content: String,
}

/// Load the config from the repo, or return the default on any non-fatal
/// failure. Never returns `Err` from the caller's perspective — a malformed
/// `.gitnodes.yml` must not take the whole app down.
pub async fn load(target: &TargetConfig, token: &str) -> Arc<BrainConfig> {
    load_with_diagnostic(target, token).await.config
}

pub async fn load_with_diagnostic(target: &TargetConfig, token: &str) -> ConfigLoadSnapshot {
    let key = TargetKey::from(target);
    if let Some(hit) = cache_get(&key) {
        return hit;
    }
    let (cfg, diagnostic) = match fetch_and_parse(target, token).await {
        Ok(Some(cfg)) => (cfg, None),
        Ok(None) => (BrainConfig::default(), None),
        Err(BrainError::Parse(message)) => {
            tracing::warn!(error = %message, "brain config invalid, using default");
            (
                BrainConfig::default(),
                Some(ConfigLoadDiagnostic { message }),
            )
        }
        Err(e) => {
            tracing::warn!(error = %e, "brain config load failed, using default");
            (BrainConfig::default(), None)
        }
    };
    let arc = Arc::new(cfg);
    cache_store(&key, arc.clone(), diagnostic.clone(), false);
    ConfigLoadSnapshot {
        config: arc,
        diagnostic,
    }
}

/// Returns `Ok(Some(cfg))` on a valid file, `Ok(None)` when neither the
/// canonical nor the legacy file exists, and `Err` on any other unrecoverable
/// error. A malformed or validation-failing YAML is reported as `Err` and the
/// caller logs+falls back to default.
///
/// Tries `.gitnodes.yml` first; only if it is absent does it fall back to the
/// pre-rename `.brain-config.yml`. A parse error on the canonical file is
/// surfaced as-is rather than masked by the legacy probe.
async fn fetch_and_parse(
    target: &TargetConfig,
    token: &str,
) -> Result<Option<BrainConfig>, BrainError> {
    match fetch_path(target, token, CONFIG_PATH).await? {
        Some(cfg) => Ok(Some(cfg)),
        None => fetch_path(target, token, LEGACY_CONFIG_PATH).await,
    }
}

/// Fetch and parse a single config path. `Ok(None)` means a 404 for that path.
async fn fetch_path(
    target: &TargetConfig,
    token: &str,
    path: &str,
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
    let url = gh.contents_url(path);
    let resp = http
        .get(&url, token)
        .query(&[("ref", target.branch.as_str())])
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

        cache_store(&a, Arc::new(BrainConfig::default()), None, false);
        assert!(cache_get(&a).is_some());
        assert!(cache_get(&b).is_none());

        invalidate(&a);
        assert!(cache_get(&a).is_none());
    }

    #[test]
    fn seeded_entry_survives_invalidate_within_grace() {
        let k = TargetKey::from(&target("o", "cfg_seed_grace", "main"));
        invalidate(&k);

        store(&k, BrainConfig::default());
        assert!(cache_get(&k).is_some(), "store seeds the cache");

        invalidate(&k);
        assert!(
            cache_get(&k).is_some(),
            "seed survives invalidate inside grace window"
        );
    }

    #[test]
    fn unseeded_entry_is_invalidated_normally() {
        let k = TargetKey::from(&target("o", "cfg_unseeded", "main"));
        invalidate(&k);

        cache_store(&k, Arc::new(BrainConfig::default()), None, false);
        assert!(cache_get(&k).is_some());

        invalidate(&k);
        assert!(
            cache_get(&k).is_none(),
            "unseeded entries (placed by load) are removed by invalidate"
        );
    }

    #[test]
    fn cache_retains_parse_diagnostic_until_invalidated() {
        let k = TargetKey::from(&target("o", "cfg_diag", "main"));
        invalidate(&k);

        cache_store(
            &k,
            Arc::new(BrainConfig::default()),
            Some(ConfigLoadDiagnostic {
                message: "node_types: invalid".to_string(),
            }),
            false,
        );

        let hit = cache_get(&k).expect("diagnostic entry cached");
        assert_eq!(
            hit.diagnostic.as_ref().map(|d| d.message.as_str()),
            Some("node_types: invalid")
        );

        invalidate(&k);
        assert!(cache_get(&k).is_none());
    }
}
