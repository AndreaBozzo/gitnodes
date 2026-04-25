use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use super::sse::EventBus;
use crate::server::sse::BrainEvent;

/// Axum state threaded into the webhook handler.
#[derive(Clone)]
pub struct WebhookState {
    pub bus: EventBus,
    pub target: brain_domain::TargetConfig,
    pub http: brain_storage::GithubHttp,
    /// Shared secret configured in the GitHub webhook settings.
    /// When `None` the endpoint is open — suitable for local dev, dangerous in prod.
    pub secret: Option<String>,
}

/// POST /webhook/github
///
/// Validates X-Hub-Signature-256, dispatches projection rebuild for `push`
/// events, then fans out a typed SSE event to connected clients.
///
/// Other event types (issues, pull_request, …) are accepted silently so
/// GitHub's delivery log doesn't flag them as failures.
pub async fn handle(
    State(state): State<WebhookState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    if let Some(ref secret) = state.secret
        && !verify_signature(secret.as_bytes(), &headers, &body)
    {
        tracing::warn!("webhook: HMAC validation failed");
        return StatusCode::UNAUTHORIZED;
    }

    let event_type = headers
        .get("X-GitHub-Event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    tracing::debug!(event_type, "webhook received");

    if event_type == "push" {
        let state_clone = state.clone();
        tokio::spawn(async move {
            handle_push(state_clone).await;
        });
    }

    StatusCode::ACCEPTED
}

async fn handle_push(state: WebhookState) {
    use brain_domain::TargetKey;
    use brain_storage::GithubStorage;

    let storage = GithubStorage::new(state.http, state.target.clone());

    // We need a token for the GitHub API calls. Webhooks arrive without a user
    // session, so we use the server-side bot token (GITHUB_TOKEN env var).
    // If it is absent we skip the rebuild — the client's next manual refresh or
    // page load will reconcile.
    let token =
        match std::env::var("GITHUB_TOKEN").or_else(|_| std::env::var("TARGET_GITHUB_TOKEN")) {
            Ok(t) => t,
            Err(_) => {
                tracing::warn!("webhook push: no GITHUB_TOKEN — skipping projection rebuild");
                state.bus.send(BrainEvent::GraphStale);
                return;
            }
        };

    let config = crate::knowledge::config_loader::load(storage.target(), &token).await;
    let key = TargetKey::from(storage.target());

    match crate::server::projection::rebuild(&storage, &token, &config, "webhook_push").await {
        Ok(()) => {
            tracing::info!(target = %key, "webhook push: projection rebuilt");
            state.bus.send(BrainEvent::GraphUpdated);
        }
        Err(error) => {
            tracing::warn!(target = %key, error = %error, "webhook push: projection rebuild failed");
            state.bus.send(BrainEvent::GraphStale);
        }
    }
}

/// Constant-time HMAC-SHA256 verification of `X-Hub-Signature-256`.
fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

fn verify_signature(secret: &[u8], headers: &HeaderMap, body: &[u8]) -> bool {
    let Some(sig_header) = headers.get("X-Hub-Signature-256") else {
        return false;
    };
    let Ok(sig_str) = sig_header.to_str() else {
        return false;
    };
    let Some(hex_sig) = sig_str.strip_prefix("sha256=") else {
        return false;
    };
    let Some(expected) = decode_hex(hex_sig) else {
        return false;
    };

    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sign(secret: &[u8], body: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap();
        mac.update(body);
        let result = mac.finalize().into_bytes();
        let hex: String = result.iter().map(|b| format!("{b:02x}")).collect();
        format!("sha256={hex}")
    }

    #[test]
    fn valid_signature_is_accepted() {
        let secret = b"test-secret";
        let body = b"hello world";
        let sig = sign(secret, body);

        let mut headers = HeaderMap::new();
        headers.insert("X-Hub-Signature-256", sig.parse().unwrap());

        assert!(verify_signature(secret, &headers, body));
    }

    #[test]
    fn wrong_secret_is_rejected() {
        let body = b"hello world";
        let sig = sign(b"correct-secret", body);

        let mut headers = HeaderMap::new();
        headers.insert("X-Hub-Signature-256", sig.parse().unwrap());

        assert!(!verify_signature(b"wrong-secret", &headers, body));
    }

    #[test]
    fn missing_signature_header_is_rejected() {
        let headers = HeaderMap::new();
        assert!(!verify_signature(b"secret", &headers, b"body"));
    }

    #[test]
    fn malformed_header_is_rejected() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Hub-Signature-256",
            "not-sha256-prefixed".parse().unwrap(),
        );
        assert!(!verify_signature(b"secret", &headers, b"body"));
    }
}
