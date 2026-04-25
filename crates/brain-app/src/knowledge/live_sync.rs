use leptos::prelude::*;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SyncStatus {
    #[default]
    Fresh,
    Stale {
        message: Option<String>,
    },
}

impl SyncStatus {
    pub fn is_stale(&self) -> bool {
        matches!(self, Self::Stale { .. })
    }
}

/// Subscribes the current page to `/sse/events` and bumps `graph_version` when
/// the backend signals a graph update. SSR is a no-op — EventSource only runs
/// in the browser after hydration.
#[component]
pub fn LiveSync(graph_version: RwSignal<u64>, sync_status: RwSignal<SyncStatus>) -> impl IntoView {
    #[cfg(feature = "hydrate")]
    {
        #[derive(serde::Deserialize)]
        struct SyncFailedPayload {
            message: Option<String>,
        }

        use wasm_bindgen::JsCast;
        use wasm_bindgen::closure::Closure;
        use web_sys::{EventSource, MessageEvent};

        let Ok(source) = EventSource::new("/sse/events") else {
            return;
        };

        let bump_updated: Closure<dyn FnMut(MessageEvent)> =
            Closure::new(move |_event: MessageEvent| {
                graph_version.update(|v| *v += 1);
                sync_status.set(SyncStatus::Fresh);
            });
        let bump_stale: Closure<dyn FnMut(MessageEvent)> =
            Closure::new(move |_event: MessageEvent| {
                graph_version.update(|v| *v += 1);
                sync_status.set(SyncStatus::Stale { message: None });
            });
        let bump_failed: Closure<dyn FnMut(MessageEvent)> =
            Closure::new(move |event: MessageEvent| {
                let message = event
                    .data()
                    .as_string()
                    .and_then(|raw| serde_json::from_str::<SyncFailedPayload>(&raw).ok())
                    .and_then(|payload| payload.message);
                graph_version.update(|v| *v += 1);
                sync_status.set(SyncStatus::Stale { message });
            });

        let _ = source.add_event_listener_with_callback(
            "graph_updated",
            bump_updated.as_ref().unchecked_ref(),
        );
        let _ = source
            .add_event_listener_with_callback("graph_stale", bump_stale.as_ref().unchecked_ref());
        let _ = source
            .add_event_listener_with_callback("sync_failed", bump_failed.as_ref().unchecked_ref());

        // Hand the closures to the JS side; they must outlive the component.
        // The EventSource itself is closed on cleanup so the callbacks can be
        // dropped safely with the component.
        bump_updated.forget();
        bump_stale.forget();
        bump_failed.forget();

        on_cleanup(move || {
            source.close();
        });
    }

    #[cfg(not(feature = "hydrate"))]
    {
        let _ = (graph_version, sync_status);
    }
}
