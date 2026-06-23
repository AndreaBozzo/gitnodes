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

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use super::sse::EventBus;
use crate::server::sse::BrainEvent;
use gitnodes_domain::{
    BrainConfig, TargetConfig, TargetRef, WorkItem, WorkItemState, WorkItemSystemOfRecord,
};

/// Axum state threaded into the webhook handler.
#[derive(Clone)]
pub struct WebhookState {
    pub bus: EventBus,
    pub http: gitnodes_storage::GithubHttp,
    pub auth: WebhookAuth,
}

#[derive(Clone)]
pub enum WebhookAuth {
    Disabled,
    Insecure,
    Secret(String),
}

impl WebhookAuth {
    fn authorize(&self, headers: &HeaderMap, body: &[u8]) -> Result<(), StatusCode> {
        match self {
            Self::Disabled => Err(StatusCode::NOT_FOUND),
            Self::Insecure => Ok(()),
            Self::Secret(secret) if verify_signature(secret.as_bytes(), headers, body) => Ok(()),
            Self::Secret(_) => Err(StatusCode::UNAUTHORIZED),
        }
    }
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
    if let Err(status) = state.auth.authorize(&headers, &body) {
        if status == StatusCode::UNAUTHORIZED {
            tracing::warn!("webhook: HMAC validation failed");
        }
        return status;
    }

    let event_type = headers
        .get("X-GitHub-Event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    tracing::debug!(event_type, "webhook received");

    if event_type == "push" {
        let Some(payload) = parse_push_payload(&body) else {
            tracing::debug!("webhook push: payload missing repository/ref — ignored");
            return StatusCode::ACCEPTED;
        };
        let Some(target) = resolve_push_target(&payload).await else {
            return StatusCode::ACCEPTED;
        };
        let state_clone = state.clone();
        spawn_supervised("handle_push", async move {
            handle_push(state_clone, target).await;
        });
    } else if event_type == "issues" || event_type == "pull_request" {
        let Some(payload) = parse_item_payload(&body, event_type) else {
            tracing::debug!(
                event_type,
                "webhook item event: payload not parseable — ignored"
            );
            return StatusCode::ACCEPTED;
        };
        let Some(target) = resolve_repo_target(&payload.repo_full_name, "webhook_item").await
        else {
            return StatusCode::ACCEPTED;
        };
        let state_clone = state.clone();
        let payload_clone = payload;
        spawn_supervised("handle_item_event", async move {
            handle_item_event(state_clone, payload_clone, target).await;
        });
    }

    StatusCode::ACCEPTED
}

/// Spawn a background webhook task and supervise it: if the task panics, the
/// `JoinError` is logged explicitly with the task name instead of vanishing
/// into a detached `tokio::spawn`. The work itself stays fire-and-forget (the
/// webhook has already returned 202); this just buys operational visibility.
/// Ties into the "Background job/outbox" item of the Failure-Mode Matrix.
fn spawn_supervised<F>(name: &'static str, fut: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let handle = tokio::spawn(fut);
    tokio::spawn(async move {
        if let Err(join_err) = handle.await {
            if join_err.is_panic() {
                tracing::error!(task = name, "webhook background task panicked");
            } else {
                tracing::warn!(task = name, "webhook background task cancelled");
            }
        }
    });
}

#[derive(serde::Deserialize)]
struct PushPayload {
    r#ref: String,
    repository: PushRepository,
}

#[derive(serde::Deserialize)]
struct PushRepository {
    full_name: String,
}

fn parse_push_payload(body: &[u8]) -> Option<PushPayload> {
    serde_json::from_slice(body).ok()
}

fn split_repo_full_name(full_name: &str) -> Option<(&str, &str)> {
    full_name.split_once('/')
}

fn branch_from_ref(r#ref: &str) -> Option<&str> {
    r#ref.strip_prefix("refs/heads/")
}

fn target_ref_from_push(payload: &PushPayload) -> Option<TargetRef> {
    let (org, repo) = split_repo_full_name(&payload.repository.full_name)?;
    let branch = branch_from_ref(&payload.r#ref)?;
    let target = TargetRef::new(org, repo, branch);
    target.validate().ok()?;
    Some(target)
}

async fn resolve_push_target(payload: &PushPayload) -> Option<TargetRef> {
    let payload_target = target_ref_from_push(payload)?;
    let configured = resolve_repo_target(&payload.repository.full_name, "webhook_push").await?;
    if configured.branch != payload_target.branch {
        crate::server::audit::log(
            "webhook_unconfigured_target",
            Some("github-webhook"),
            &format!(
                "push {} ignored; configured branch is {}",
                payload_target, configured.branch
            ),
        )
        .await;
        tracing::debug!(
            payload_target = %payload_target,
            configured_target = %configured,
            "webhook push: branch mismatch — ignored"
        );
        return None;
    }
    Some(configured)
}

async fn resolve_repo_target(repo_full_name: &str, reason: &str) -> Option<TargetRef> {
    let (org, repo) = split_repo_full_name(repo_full_name)?;
    let Some(pool) = crate::server::projection::pool_handle() else {
        crate::server::audit::log(
            "webhook_unconfigured_target",
            Some("github-webhook"),
            &format!("{reason} {repo_full_name}: projection pool unavailable"),
        )
        .await;
        return None;
    };
    match crate::server::target_registry::lookup(pool, org, repo).await {
        Ok(Some(entry)) => Some(entry.target_ref()),
        Ok(None) => {
            crate::server::audit::log(
                "webhook_unconfigured_target",
                Some("github-webhook"),
                &format!("{reason} {repo_full_name}: target not registered"),
            )
            .await;
            tracing::debug!(repo = %repo_full_name, "webhook: target not registered — ignored");
            None
        }
        Err(error) => {
            crate::server::audit::log(
                "webhook_target_lookup_error",
                Some("github-webhook"),
                &format!("{reason} {repo_full_name}: {error}"),
            )
            .await;
            tracing::warn!(%error, repo = %repo_full_name, "webhook: target lookup failed");
            None
        }
    }
}

async fn handle_push(state: WebhookState, target: TargetRef) {
    use gitnodes_domain::TargetKey;
    use gitnodes_storage::GithubStorage;

    let target_cfg = TargetConfig::from(&target);
    let storage = GithubStorage::new(state.http.clone(), target_cfg);

    // We need a token for the GitHub API calls. Webhooks arrive without a user
    // session, so we authenticate as the GitHub App (preferred) or fall back to
    // a server-side PAT. If neither is configured we skip the rebuild — the
    // client's next manual refresh or page load will reconcile.
    let token = match crate::server::installation_token::get(&state.http).await {
        Some(t) => t,
        None => {
            tracing::warn!(
                "webhook push: no GitHub App or PAT credentials — skipping projection rebuild"
            );
            state.bus.send(BrainEvent::SyncFailed {
                target,
                message: "Background sync skipped: no GitHub credentials configured on the server. Showing the last known snapshot until a manual refresh succeeds.".to_string(),
            });
            return;
        }
    };

    let key = TargetKey::from(storage.target());

    // Mirror the manual-refresh contract: a push can change `.gitnodes.yml`
    // and template files, so we must drop the per-target caches before reading
    // them. Without this, the projection rebuild can run against stale node-type
    // metadata for up to the config TTL and still broadcast `graph_updated`.
    gitnodes_storage::invalidate(&key);
    gitnodes_storage::invalidate_template(&key);
    crate::knowledge::config_loader::invalidate(&key);

    let config = crate::knowledge::config_loader::load(storage.target(), &token).await;

    match crate::server::projection::rebuild(&storage, &token, &config, "webhook_push").await {
        Ok(()) => {
            tracing::info!(target = %key, "webhook push: projection rebuilt");
            state.bus.send(BrainEvent::GraphUpdated { target });
        }
        Err(error) => {
            tracing::warn!(target = %key, error = %error, "webhook push: projection rebuild failed");
            state.bus.send(BrainEvent::SyncFailed {
                target,
                message: format!(
                    "Background sync failed after the latest GitHub push: {error}. Showing the last successful snapshot."
                ),
            });
        }
    }
}

/// Minimal payload shared by `issues` and `pull_request` webhook events: we
/// only need the repository (to gate cross-repo deliveries) and the item
/// number (to look up the bound work item). The rest of the payload is
/// authoritative on GitHub — we never trust it as the source of truth, we use
/// it only as a *trigger* to rebuild the projection from the forge.
#[derive(Clone, Debug)]
struct ItemEventPayload {
    repo_full_name: String,
    item_key: String,
    provider_state: Option<String>,
    labels: Vec<String>,
    assignees: Vec<String>,
    /// `issues` → `"issue"`. `pull_request` → `"pull_request"`. Currently
    /// unused by the handler but kept for future log/observability tagging.
    #[allow(dead_code)]
    item_kind: &'static str,
}

#[derive(serde::Deserialize)]
struct IssuesEnvelope {
    repository: PushRepository,
    issue: ItemPayload,
}

#[derive(serde::Deserialize)]
struct PullRequestEnvelope {
    repository: PushRepository,
    pull_request: ItemPayload,
}

#[derive(serde::Deserialize)]
struct ItemPayload {
    number: u64,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    labels: Vec<ItemLabel>,
    #[serde(default)]
    assignees: Vec<ItemAssignee>,
}

#[derive(serde::Deserialize)]
struct ItemLabel {
    name: String,
}

#[derive(serde::Deserialize)]
struct ItemAssignee {
    login: String,
}

fn parse_item_payload(body: &[u8], event_type: &str) -> Option<ItemEventPayload> {
    match event_type {
        "issues" => {
            let env: IssuesEnvelope = serde_json::from_slice(body).ok()?;
            Some(ItemEventPayload {
                repo_full_name: env.repository.full_name,
                item_key: env.issue.number.to_string(),
                provider_state: env.issue.state,
                labels: env
                    .issue
                    .labels
                    .into_iter()
                    .map(|label| label.name)
                    .collect(),
                assignees: env
                    .issue
                    .assignees
                    .into_iter()
                    .map(|assignee| assignee.login)
                    .collect(),
                item_kind: "issue",
            })
        }
        "pull_request" => {
            let env: PullRequestEnvelope = serde_json::from_slice(body).ok()?;
            Some(ItemEventPayload {
                repo_full_name: env.repository.full_name,
                item_key: env.pull_request.number.to_string(),
                provider_state: env.pull_request.state,
                labels: env
                    .pull_request
                    .labels
                    .into_iter()
                    .map(|label| label.name)
                    .collect(),
                assignees: env
                    .pull_request
                    .assignees
                    .into_iter()
                    .map(|assignee| assignee.login)
                    .collect(),
                item_kind: "pull_request",
            })
        }
        _ => None,
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn item_matches_target(payload: &ItemEventPayload, target: &TargetRef) -> bool {
    payload.repo_full_name == format!("{}/{}", target.org, target.repo)
}

async fn handle_item_event(state: WebhookState, payload: ItemEventPayload, target: TargetRef) {
    use gitnodes_domain::TargetKey;
    use gitnodes_storage::GithubStorage;

    // The binding lookup uses the project as `{org}/{repo}` to stay
    // forge-agnostic with how `WorkItemControls` writes it from the UI.
    let target_cfg = TargetConfig::from(&target);
    let project = format!("{}/{}", target.org, target.repo);
    let bound = match crate::server::projection::find_work_item_by_external(
        &target_cfg,
        "github",
        &project,
        &payload.item_key,
    )
    .await
    {
        Ok(Some(b)) => b,
        Ok(None) => {
            tracing::debug!(
                repo = %payload.repo_full_name,
                item_key = %payload.item_key,
                "webhook item event: no bound work item — ignored"
            );
            return;
        }
        Err(error) => {
            tracing::warn!(error = %error, "webhook item event: binding lookup failed");
            return;
        }
    };

    let token = match crate::server::installation_token::get(&state.http).await {
        Some(t) => t,
        None => {
            tracing::warn!(
                "webhook item event: no GitHub credentials — emitting cache-bump event without rebuild"
            );
            state.bus.send(BrainEvent::WorkItemUpdated {
                target,
                brain_id: bound.0,
                content_path: bound.1,
            });
            return;
        }
    };

    let storage = GithubStorage::new(state.http.clone(), target_cfg.clone());
    let key = TargetKey::from(storage.target());
    gitnodes_storage::invalidate(&key);
    gitnodes_storage::invalidate_template(&key);
    crate::knowledge::config_loader::invalidate(&key);

    let config = crate::knowledge::config_loader::load(storage.target(), &token).await;
    if let Err(error) =
        crate::server::projection::rebuild(&storage, &token, &config, "webhook_item").await
    {
        tracing::warn!(target = %key, error = %error, "webhook item event: projection rebuild failed");
        // Still emit so the UI bumps and refetches — the next manual refresh
        // or push event will reconcile.
    }

    match crate::server::projection::load_work_item_by_brain_id(&target_cfg, &bound.0).await {
        Ok(Some(item)) => {
            let next_state = provider_state_for_item(&item, &payload, &config);
            let next_assignees = (item.system_of_record != WorkItemSystemOfRecord::Brain)
                .then(|| payload.assignees.clone());
            if let Err(error) = crate::api::apply_provider_work_item_update(
                &token,
                "github-webhook",
                &target_cfg,
                &storage,
                &bound.0,
                next_state,
                next_assignees,
            )
            .await
            {
                tracing::warn!(
                    target = %key,
                    brain_id = %bound.0,
                    error = %error,
                    "webhook item event: failed to apply provider update to Brain file"
                );
                crate::server::audit::log(
                    "work_item_provider_reconcile_error",
                    Some("github-webhook"),
                    &format!("{}: {error}", bound.0),
                )
                .await;
            }
        }
        Ok(None) => {}
        Err(error) => {
            tracing::warn!(
                target = %key,
                brain_id = %bound.0,
                error = %error,
                "webhook item event: failed to load bound work item after rebuild"
            );
        }
    }

    state.bus.send(BrainEvent::WorkItemUpdated {
        target,
        brain_id: bound.0,
        content_path: bound.1,
    });
}

fn provider_state_for_item(
    item: &WorkItem,
    payload: &ItemEventPayload,
    config: &BrainConfig,
) -> Option<WorkItemState> {
    if item.system_of_record == WorkItemSystemOfRecord::Brain {
        return None;
    }

    if let Some(spec) = config.labels_for_kind(&item.kind) {
        for (state, label) in &spec.state_labels {
            if payload.labels.iter().any(|candidate| candidate == label) {
                return Some(state.clone());
            }
        }
    }

    match payload.provider_state.as_deref() {
        Some("closed") if !matches!(item.state, WorkItemState::Done | WorkItemState::Cancelled) => {
            Some(WorkItemState::Done)
        }
        Some("open") if matches!(item.state, WorkItemState::Done | WorkItemState::Cancelled) => {
            Some(WorkItemState::Todo)
        }
        _ => None,
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
    fn disabled_webhook_is_not_exposed() {
        assert_eq!(
            WebhookAuth::Disabled.authorize(&HeaderMap::new(), b"body"),
            Err(StatusCode::NOT_FOUND)
        );
    }

    #[test]
    fn insecure_webhook_is_explicitly_allowed() {
        assert_eq!(
            WebhookAuth::Insecure.authorize(&HeaderMap::new(), b"body"),
            Ok(())
        );
    }

    #[test]
    fn push_payload_extracts_target_ref() {
        let body = br#"{"ref":"refs/heads/main","repository":{"full_name":"acme/brain"}}"#;
        let payload = parse_push_payload(body).unwrap();
        assert_eq!(
            target_ref_from_push(&payload),
            Some(TargetRef::new("acme", "brain", "main"))
        );
    }

    #[test]
    fn push_payload_preserves_branch_slashes() {
        let body = br#"{"ref":"refs/heads/feature/foo","repository":{"full_name":"acme/brain"}}"#;
        let payload = parse_push_payload(body).unwrap();
        assert_eq!(
            target_ref_from_push(&payload),
            Some(TargetRef::new("acme", "brain", "feature/foo"))
        );
    }

    #[test]
    fn non_branch_push_has_no_target_ref() {
        let body = br#"{"ref":"refs/tags/v1","repository":{"full_name":"acme/brain"}}"#;
        let payload = parse_push_payload(body).unwrap();
        assert!(target_ref_from_push(&payload).is_none());
    }

    #[test]
    fn payload_without_ref_is_ignored() {
        let body = br#"{"repository":{"full_name":"acme/brain"}}"#;
        assert!(parse_push_payload(body).is_none());
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

    #[test]
    fn issues_payload_parses_repo_and_number() {
        let body = br#"{"action":"labeled","issue":{"number":42,"state":"open","labels":[{"name":"brain:blocked"}],"assignees":[{"login":"alice"}]},"repository":{"full_name":"acme/brain"}}"#;
        let payload = parse_item_payload(body, "issues").unwrap();
        assert_eq!(payload.repo_full_name, "acme/brain");
        assert_eq!(payload.item_key, "42");
        assert_eq!(payload.provider_state.as_deref(), Some("open"));
        assert_eq!(payload.labels, vec!["brain:blocked".to_string()]);
        assert_eq!(payload.assignees, vec!["alice".to_string()]);
        assert_eq!(payload.item_kind, "issue");
    }

    #[test]
    fn pull_request_payload_parses_repo_and_number() {
        let body = br#"{"action":"opened","pull_request":{"number":7},"repository":{"full_name":"acme/brain"}}"#;
        let payload = parse_item_payload(body, "pull_request").unwrap();
        assert_eq!(payload.item_key, "7");
        assert_eq!(payload.item_kind, "pull_request");
    }

    #[test]
    fn item_payload_for_unrelated_repo_is_rejected() {
        let payload = ItemEventPayload {
            repo_full_name: "evil/elsewhere".to_string(),
            item_key: "1".to_string(),
            provider_state: None,
            labels: vec![],
            assignees: vec![],
            item_kind: "issue",
        };
        assert!(!item_matches_target(
            &payload,
            &TargetRef::new("acme", "brain", "main")
        ));
    }

    #[test]
    fn item_payload_for_target_repo_is_accepted() {
        let payload = ItemEventPayload {
            repo_full_name: "acme/brain".to_string(),
            item_key: "1".to_string(),
            provider_state: None,
            labels: vec![],
            assignees: vec![],
            item_kind: "issue",
        };
        assert!(item_matches_target(
            &payload,
            &TargetRef::new("acme", "brain", "main")
        ));
    }

    #[test]
    fn provider_state_prefers_brain_state_label() {
        let item = WorkItem {
            brain_id: "task-1".to_string(),
            kind: gitnodes_domain::WorkItemKind::Task,
            title: "Task".to_string(),
            state: WorkItemState::Todo,
            labels: vec![],
            assignees: vec![],
            content_path: Some("tasks/task.md".to_string()),
            external_binding: None,
            system_of_record: WorkItemSystemOfRecord::Split,
        };
        let payload = ItemEventPayload {
            repo_full_name: "acme/brain".to_string(),
            item_key: "1".to_string(),
            provider_state: Some("open".to_string()),
            labels: vec!["brain:blocked".to_string()],
            assignees: vec![],
            item_kind: "issue",
        };

        let state = provider_state_for_item(&item, &payload, &BrainConfig::default());
        assert_eq!(state, Some(WorkItemState::Blocked));
    }

    #[test]
    fn provider_closed_state_falls_back_to_done() {
        let item = WorkItem {
            brain_id: "task-1".to_string(),
            kind: gitnodes_domain::WorkItemKind::Task,
            title: "Task".to_string(),
            state: WorkItemState::InProgress,
            labels: vec![],
            assignees: vec![],
            content_path: Some("tasks/task.md".to_string()),
            external_binding: None,
            system_of_record: WorkItemSystemOfRecord::External,
        };
        let payload = ItemEventPayload {
            repo_full_name: "acme/brain".to_string(),
            item_key: "1".to_string(),
            provider_state: Some("closed".to_string()),
            labels: vec![],
            assignees: vec![],
            item_kind: "issue",
        };

        let state = provider_state_for_item(&item, &payload, &BrainConfig::default());
        assert_eq!(state, Some(WorkItemState::Done));
    }
}
