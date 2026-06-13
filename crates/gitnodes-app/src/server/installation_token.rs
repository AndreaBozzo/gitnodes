//! GitHub App installation-token minter.
//!
//! Webhooks arrive without a user session, so the projection rebuild has to
//! authenticate as the App itself. This module mints + caches an installation
//! access token: it signs a short-lived RS256 JWT with the App's private key,
//! exchanges it at `POST /app/installations/{id}/access_tokens`, and serves
//! the resulting `ghs_*` token until ~5 minutes before its 1-hour expiry.
//!
//! Falls back to the `GITHUB_TOKEN` / `TARGET_GITHUB_TOKEN` env var when the
//! App is not configured, so existing PAT-based deployments keep working.

use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gitnodes_domain::GithubClient;
use gitnodes_storage::GithubHttp;
use jsonwebtoken::{EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokio::sync::Mutex;

/// Refresh `expires_at - SAFETY_WINDOW` so we never hand out a token that's
/// about to die mid-request. GitHub installation tokens are valid for 60 min;
/// 5 min of headroom is generous.
const SAFETY_WINDOW: Duration = Duration::from_secs(5 * 60);

/// Failure detail for the App installation-token mint step. The App path falls
/// back to a PAT on failure, so this never escapes [`get`] — it's the internal
/// error type for [`mint`], surfaced only in logs.
#[derive(Debug)]
pub enum TokenMintError {
    /// Minting the installation token failed. Carries the underlying detail
    /// (JWT sign, HTTP status + snippet, parse, …).
    MintFailed(String),
}

impl std::fmt::Display for TokenMintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenMintError::MintFailed(detail) => {
                write!(f, "installation token mint failed: {detail}")
            }
        }
    }
}

impl std::error::Error for TokenMintError {}

#[derive(Clone)]
struct AppConfig {
    app_id: String,
    installation_id: String,
    api_base: String,
    private_key_pem: Vec<u8>,
}

#[derive(Clone)]
struct CachedToken {
    token: String,
    expires_at: SystemTime,
}

struct Cache {
    config: Option<AppConfig>,
    cached: Option<CachedToken>,
}

static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();

fn cache() -> &'static Mutex<Cache> {
    CACHE.get_or_init(|| {
        Mutex::new(Cache {
            config: load_config_from_env(),
            cached: None,
        })
    })
}

fn load_config_from_env() -> Option<AppConfig> {
    let app_id = std::env::var("GITHUB_APP_ID").ok()?;
    let installation_id = std::env::var("GITHUB_APP_INSTALLATION_ID").ok()?;

    // Prefer inline PEM (literal `\n` allowed for single-line .env values),
    // fall back to a path on disk for k8s-style secret mounts.
    let pem = if let Ok(raw) = std::env::var("GITHUB_APP_PRIVATE_KEY") {
        raw.replace("\\n", "\n").into_bytes()
    } else if let Ok(path) = std::env::var("GITHUB_APP_PRIVATE_KEY_PATH") {
        match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                tracing::warn!(%path, %error, "GITHUB_APP_PRIVATE_KEY_PATH unreadable");
                return None;
            }
        }
    } else {
        return None;
    };

    if app_id.trim().is_empty() || installation_id.trim().is_empty() || pem.is_empty() {
        return None;
    }

    let api_base = std::env::var("GITHUB_API_BASE")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| GithubClient::default_api_base().to_string());

    Some(AppConfig {
        app_id,
        installation_id,
        api_base,
        private_key_pem: pem,
    })
}

/// Get a token usable as a Bearer for the GitHub REST API.
///
/// Resolution order:
///   1. Cached App installation token (if still fresh).
///   2. Mint a new App installation token.
///   3. Fall back to `GITHUB_TOKEN` / `TARGET_GITHUB_TOKEN` env var (PAT mode).
///
/// Returns `None` only when no usable credential resolves — either nothing is
/// configured, or the App mint failed *and* no PAT fallback exists. Mint
/// failures are logged and fall through to the PAT path rather than surfacing as
/// an error, so callers only ever see "a token" or "no usable credential".
pub async fn get(http: &GithubHttp) -> Option<String> {
    {
        let mut guard = cache().lock().await;
        if guard.config.is_some() {
            if let Some(cached) = &guard.cached
                && cached
                    .expires_at
                    .duration_since(SystemTime::now())
                    .map(|left| left > SAFETY_WINDOW)
                    .unwrap_or(false)
            {
                return Some(cached.token.clone());
            }
            // Drop the stale entry before we await on the network so a
            // concurrent caller can also see we need to refresh. The mint call
            // itself is cheap and idempotent enough that double-minting in a
            // race is harmless.
            guard.cached = None;
        }
    }

    let config = {
        let guard = cache().lock().await;
        guard.config.clone()
    };

    if let Some(config) = config {
        match mint(http, &config).await {
            Ok(fresh) => {
                tracing::info!(
                    auth_tier = "app",
                    "github auth: minted fresh installation token"
                );
                let mut guard = cache().lock().await;
                guard.cached = Some(fresh.clone());
                return Some(fresh.token);
            }
            Err(error) => {
                tracing::warn!(%error, "github app token mint failed — falling back to PAT");
                // Fall through to PAT fallback.
            }
        }
    }

    // PAT fallback. Kept so existing dev setups (and the deploy currently
    // running on a fine-grained PAT) keep working until the App rollout
    // finishes.
    if let Ok(pat) = std::env::var("GITHUB_TOKEN").or_else(|_| std::env::var("TARGET_GITHUB_TOKEN"))
    {
        let trimmed = pat.trim();
        if !trimmed.is_empty() {
            tracing::info!(auth_tier = "pat", "github auth: using PAT fallback");
            return Some(trimmed.to_string());
        }
    }

    tracing::info!(auth_tier = "none", "github auth: no credentials configured");
    None
}

#[derive(Serialize)]
struct JwtClaims {
    iat: u64,
    exp: u64,
    iss: String,
}

#[derive(Deserialize)]
struct InstallationTokenResponse {
    token: String,
    expires_at: String,
}

async fn mint(http: &GithubHttp, config: &AppConfig) -> Result<CachedToken, TokenMintError> {
    use TokenMintError::MintFailed;
    // App JWT: short-lived (≤ 10 min by GitHub policy). We use 9 to absorb
    // clock skew on either side.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| MintFailed(format!("system time before unix epoch: {e}")))?
        .as_secs();
    let claims = JwtClaims {
        iat: now.saturating_sub(60),
        exp: now + 9 * 60,
        iss: config.app_id.clone(),
    };
    let key = EncodingKey::from_rsa_pem(&config.private_key_pem)
        .map_err(|e| MintFailed(format!("private key parse: {e}")))?;
    let jwt = encode(&Header::new(jsonwebtoken::Algorithm::RS256), &claims, &key)
        .map_err(|e| MintFailed(format!("jwt sign: {e}")))?;

    let url =
        GithubClient::app_installation_access_tokens_url(&config.api_base, &config.installation_id);

    let resp = http
        .post(&url, &jwt)
        .send()
        .await
        .map_err(|e| MintFailed(format!("installation token request: {e}")))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        let snippet: String = body.chars().take(400).collect();
        return Err(MintFailed(format!(
            "installation token status {status}: {snippet}"
        )));
    }

    let parsed: InstallationTokenResponse = resp
        .json()
        .await
        .map_err(|e| MintFailed(format!("installation token parse: {e}")))?;

    let expires_at = OffsetDateTime::parse(
        &parsed.expires_at,
        &time::format_description::well_known::Rfc3339,
    )
    .map(|dt| SystemTime::UNIX_EPOCH + Duration::from_secs(dt.unix_timestamp() as u64))
    .unwrap_or_else(|_| SystemTime::now() + Duration::from_secs(55 * 60));

    tracing::info!(
        installation_id = %config.installation_id,
        expires_at = %parsed.expires_at,
        "github app installation token minted"
    );

    Ok(CachedToken {
        token: parsed.token,
        expires_at,
    })
}
