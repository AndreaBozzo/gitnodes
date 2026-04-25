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
    /// The projection rebuild failed; the frontend should show a stale banner.
    GraphStale,
}

impl BrainEvent {
    fn as_event_name(&self) -> &'static str {
        match self {
            BrainEvent::GraphUpdated => "graph_updated",
            BrainEvent::GraphStale => "graph_stale",
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
        let sse_event = Event::default().event(event.as_event_name()).data("{}");
        Some(Ok::<Event, Infallible>(sse_event))
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}
