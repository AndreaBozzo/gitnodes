use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
};
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Mutex, OnceLock};
use tokio::sync::broadcast;
use tokio_stream::{StreamExt, wrappers::BroadcastStream};

use brain_domain::{TargetKey, TargetRef};

/// Per-target channel capacity. Each target now gets its own broadcast channel
/// (slice δ), so a single noisy target can no longer evict another target's
/// events from a shared ring buffer. 64 slots per target is generous for the
/// burst of events a single push/rebuild produces.
const CHANNEL_CAPACITY: usize = 64;
/// Hard cap on simultaneously tracked target channels. Valid target parsing
/// stops arbitrary garbage keys; this cap stops an authenticated client from
/// growing the map forever with many valid-looking branch names.
const MAX_TARGET_CHANNELS: usize = 512;

static EVENT_BUS: OnceLock<EventBus> = OnceLock::new();

/// Register the process-wide event bus. Call once during startup, after
/// constructing the `EventBus` that the webhook + SSE handlers will share.
/// Safe to call only once; subsequent calls are no-ops.
pub fn init(bus: EventBus) {
    let _ = EVENT_BUS.set(bus);
}

/// Read the process-wide event bus. Returns `None` before [`init`] runs (e.g.
/// in tests that don't bring up the full server). Mutate-only server fns use
/// this to publish granular SSE events without taking the bus through every
/// call site.
pub fn global() -> Option<&'static EventBus> {
    EVENT_BUS.get()
}

/// Typed events the backend can publish to all connected Leptos clients.
#[derive(Clone, Debug)]
pub enum BrainEvent {
    /// A push to the target repo was processed and the projection was rebuilt.
    GraphUpdated { target: TargetRef },
    /// The background sync worker failed and can provide an operator-facing reason.
    SyncFailed { target: TargetRef, message: String },
    /// A single work item mutated (state, assignees, labels). Carries enough
    /// identity for the client to refetch a single record without a full graph
    /// reload, but clients that don't handle it can fall back to bumping the
    /// generic graph version.
    WorkItemUpdated {
        target: TargetRef,
        brain_id: String,
        content_path: Option<String>,
    },
    /// The external binding of a work item changed (set, cleared, or rebound).
    BindingUpdated {
        target: TargetRef,
        brain_id: String,
        content_path: Option<String>,
    },
}

impl BrainEvent {
    /// The target every event is scoped to. Used to route the event to that
    /// target's broadcast channel (slice δ).
    fn target(&self) -> &TargetRef {
        match self {
            BrainEvent::GraphUpdated { target }
            | BrainEvent::SyncFailed { target, .. }
            | BrainEvent::WorkItemUpdated { target, .. }
            | BrainEvent::BindingUpdated { target, .. } => target,
        }
    }

    fn as_event_name(&self) -> &'static str {
        match self {
            BrainEvent::GraphUpdated { .. } => "graph_updated",
            BrainEvent::SyncFailed { .. } => "sync_failed",
            BrainEvent::WorkItemUpdated { .. } => "work_item_updated",
            BrainEvent::BindingUpdated { .. } => "binding_updated",
        }
    }

    fn as_event_data(&self) -> String {
        match self {
            BrainEvent::GraphUpdated { target } => {
                serde_json::json!({ "target": target }).to_string()
            }
            BrainEvent::SyncFailed { target, message } => {
                serde_json::json!({ "target": target, "message": message }).to_string()
            }
            BrainEvent::WorkItemUpdated {
                target,
                brain_id,
                content_path,
            }
            | BrainEvent::BindingUpdated {
                target,
                brain_id,
                content_path,
            } => serde_json::json!({
                "target": target,
                "brain_id": brain_id,
                "content_path": content_path,
            })
            .to_string(),
        }
    }
}

/// Per-target broadcast registry (slice δ). Each target gets its own
/// `broadcast::Sender`, created lazily on first send/subscribe, so a noisy
/// target can't starve others on a single shared 64-slot ring and the server
/// delivers only the requested target's events instead of relying on JS-side
/// filtering. Cloning is cheap — the `Arc` is shared.
///
/// Concurrency follows the codebase idiom (`OnceLock<Mutex<HashMap<TargetKey,
/// _>>>` as in `brain_storage`'s caches) rather than pulling in `dashmap`:
/// send/subscribe are low-frequency, so the brief lock is uncontended.
#[derive(Clone)]
pub struct EventBus {
    channels: std::sync::Arc<Mutex<HashMap<TargetKey, broadcast::Sender<BrainEvent>>>>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            channels: std::sync::Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get-or-create the sender for one target.
    fn sender_for(&self, key: TargetKey) -> Option<broadcast::Sender<BrainEvent>> {
        let mut map = self.channels.lock().expect("sse channel map poisoned");
        if !map.contains_key(&key) && map.len() >= MAX_TARGET_CHANNELS {
            map.retain(|_, sender| sender.receiver_count() > 0);
            if map.len() >= MAX_TARGET_CHANNELS {
                return None;
            }
        }
        Some(
            map.entry(key)
                .or_insert_with(|| broadcast::channel(CHANNEL_CAPACITY).0)
                .clone(),
        )
    }

    /// Publish an event to its target's channel only.
    pub fn send(&self, event: BrainEvent) {
        let key = TargetKey::from(event.target());
        // Ignore errors: zero receivers is normal when no client is connected
        // to this target.
        if let Some(sender) = self.sender_for(key.clone()) {
            let _ = sender.send(event);
        } else {
            tracing::warn!(target = %key, "sse channel cap reached; dropping event");
        }
    }

    /// Subscribe to a single target's stream.
    pub fn subscribe(&self, key: TargetKey) -> Option<broadcast::Receiver<BrainEvent>> {
        self.sender_for(key).map(|sender| sender.subscribe())
    }

    #[cfg(test)]
    fn channel_count(&self) -> usize {
        self.channels
            .lock()
            .expect("sse channel map poisoned")
            .len()
    }
}

/// SSE handler state: the bus plus the env-default target key, used when a
/// client connects without an explicit `?target=` (the legacy single-target
/// deploy, where the client can't derive org/repo/branch from a bare
/// `/knowledge` path).
#[derive(Clone)]
pub struct SseState {
    pub bus: EventBus,
    pub default_target: TargetKey,
}

#[derive(serde::Deserialize)]
pub struct SseParams {
    /// `org/repo/branch` of the target the client wants events for. Absent on
    /// legacy single-target deploys.
    target: Option<String>,
}

/// GET /sse/events?target=org/repo/branch
///
/// Long-lived SSE stream scoped to a single target (slice δ). The client passes
/// the active target; the server subscribes only to that target's channel, so
/// events from other targets never reach this connection. Falls back to the
/// env-default target when no param is given.
pub async fn handle(State(state): State<SseState>, Query(params): Query<SseParams>) -> Response {
    let key = match params.target.filter(|t| !t.is_empty()) {
        Some(target) => match TargetKey::try_from_key_string(&target) {
            Ok(key) => key,
            Err(error) => {
                return (
                    StatusCode::BAD_REQUEST,
                    format!("invalid SSE target: {error}"),
                )
                    .into_response();
            }
        },
        None => state.default_target.clone(),
    };

    let Some(rx) = state.bus.subscribe(key) else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "too many active SSE targets; retry later",
        )
            .into_response();
    };

    let stream = BroadcastStream::new(rx).filter_map(|result| {
        let event = result.ok()?;
        let sse_event = Event::default()
            .event(event.as_event_name())
            .data(event.as_event_data());
        Some(Ok::<Event, Infallible>(sse_event))
    });

    // X-Accel-Buffering: no tells nginx/Railway's proxy not to buffer this
    // stream. Without it the proxy holds chunks until its buffer fills, then
    // closes the HTTP/2 stream with ERR_HTTP2_PROTOCOL_ERROR.
    (
        [("X-Accel-Buffering", "no")],
        Sse::new(stream).keep_alive(KeepAlive::default()),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(org: &str, repo: &str, branch: &str) -> TargetRef {
        TargetRef::new(org, repo, branch)
    }

    #[tokio::test]
    async fn events_are_isolated_per_target() {
        let bus = EventBus::new();
        let a = target("Org", "RepoA", "main");
        let b = target("Org", "RepoB", "main");

        let mut rx_a = bus.subscribe(TargetKey::from(&a)).unwrap();
        let mut rx_b = bus.subscribe(TargetKey::from(&b)).unwrap();

        // A push on target A must reach A's subscriber only.
        bus.send(BrainEvent::GraphUpdated { target: a.clone() });

        let got_a = rx_a
            .try_recv()
            .expect("A subscriber should receive A's event");
        assert!(matches!(got_a, BrainEvent::GraphUpdated { target } if target == a));
        assert!(
            rx_b.try_recv().is_err(),
            "B subscriber must not see A's event"
        );
    }

    #[tokio::test]
    async fn second_subscriber_to_same_target_shares_channel() {
        let bus = EventBus::new();
        let t = target("Org", "Repo", "main");
        let mut rx1 = bus.subscribe(TargetKey::from(&t)).unwrap();
        let mut rx2 = bus.subscribe(TargetKey::from(&t)).unwrap();

        bus.send(BrainEvent::WorkItemUpdated {
            target: t.clone(),
            brain_id: "wi-1".into(),
            content_path: None,
        });

        assert!(rx1.try_recv().is_ok());
        assert!(rx2.try_recv().is_ok(), "both subscribers see the event");
    }

    #[tokio::test]
    async fn send_with_no_subscriber_is_silent() {
        let bus = EventBus::new();
        // No panic / no error even though nobody is listening on this target.
        bus.send(BrainEvent::GraphUpdated {
            target: target("Org", "Repo", "main"),
        });
    }

    #[tokio::test]
    async fn handler_rejects_invalid_target_param_without_creating_channel() {
        let bus = EventBus::new();
        let state = SseState {
            bus: bus.clone(),
            default_target: TargetKey::from(&target("Org", "Repo", "main")),
        };

        let response = handle(
            State(state),
            Query(SseParams {
                target: Some("Org/Repo/../escape".to_string()),
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(bus.channel_count(), 0);
    }

    #[tokio::test]
    async fn handler_accepts_branch_names_with_slashes() {
        let bus = EventBus::new();
        let state = SseState {
            bus: bus.clone(),
            default_target: TargetKey::from(&target("Org", "Repo", "main")),
        };

        let response = handle(
            State(state),
            Query(SseParams {
                target: Some("Org/Repo/feature/foo".to_string()),
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(bus.channel_count(), 1);
    }
}
