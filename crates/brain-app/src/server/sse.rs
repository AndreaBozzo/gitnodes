use axum::{
    extract::State,
    response::{
        Sse,
        sse::{Event, KeepAlive},
    },
};
use std::convert::Infallible;
use tokio::sync::broadcast;
use tokio_stream::{Stream, StreamExt, wrappers::BroadcastStream};

/// Typed events the backend can publish to all connected Leptos clients.
#[derive(Clone, Debug)]
pub enum BrainEvent {
    /// A push to the target repo was processed and the projection was rebuilt.
    GraphUpdated,
    /// The background sync worker failed and can provide an operator-facing reason.
    SyncFailed { message: String },
}

impl BrainEvent {
    fn as_event_name(&self) -> &'static str {
        match self {
            BrainEvent::GraphUpdated => "graph_updated",
            BrainEvent::SyncFailed { .. } => "sync_failed",
        }
    }

    fn as_event_data(&self) -> String {
        match self {
            BrainEvent::GraphUpdated => "{}".to_string(),
            BrainEvent::SyncFailed { message } => {
                serde_json::json!({ "message": message }).to_string()
            }
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
pub async fn handle(
    State(bus): State<EventBus>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = bus.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| {
        let event = result.ok()?;
        let sse_event = Event::default()
            .event(event.as_event_name())
            .data(event.as_event_data());
        Some(Ok::<Event, Infallible>(sse_event))
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}
