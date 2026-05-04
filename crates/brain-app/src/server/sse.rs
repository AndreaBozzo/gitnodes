use axum::{
    extract::State,
    response::{
        IntoResponse, Sse,
        sse::{Event, KeepAlive},
    },
};
use std::convert::Infallible;
use std::sync::OnceLock;
use tokio::sync::broadcast;
use tokio_stream::{StreamExt, wrappers::BroadcastStream};

use brain_domain::TargetRef;

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

/// Shared broadcast channel. Cloning this is cheap — it just clones the sender.
#[derive(Clone)]
pub struct EventBus(broadcast::Sender<BrainEvent>);

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(64);
        Self(tx)
    }

    pub fn send(&self, event: BrainEvent) {
        // Ignore errors: zero receivers is normal when no client is connected.
        let _ = self.0.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<BrainEvent> {
        self.0.subscribe()
    }
}

/// GET /sse/events
///
/// Long-lived SSE stream. Each connected Leptos client gets typed events so it
/// can react without a full page reload.
pub async fn handle(State(bus): State<EventBus>) -> impl IntoResponse {
    let rx = bus.subscribe();
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
}
