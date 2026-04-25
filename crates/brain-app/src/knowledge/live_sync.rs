use leptos::prelude::*;

/// Subscribes the current page to `/sse/events` and bumps `graph_version` when
/// the backend signals a graph update. SSR is a no-op — EventSource only runs
/// in the browser after hydration.
#[component]
pub fn LiveSync(graph_version: RwSignal<u64>) -> impl IntoView {
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::JsCast;
        use wasm_bindgen::closure::Closure;
        use web_sys::{EventSource, MessageEvent};

        let Ok(source) = EventSource::new("/sse/events") else {
            return;
        };

        // The webhook handler emits both `graph_updated` (rebuild succeeded)
        // and `graph_stale` (rebuild failed). For now we treat both the same:
        // re-fetch from the server so it can serve either the fresh snapshot
        // or the last good one. Future work: surface a stale banner.
        let bump_updated: Closure<dyn FnMut(MessageEvent)> =
            Closure::new(move |_event: MessageEvent| {
                graph_version.update(|v| *v += 1);
            });
        let bump_stale: Closure<dyn FnMut(MessageEvent)> =
            Closure::new(move |_event: MessageEvent| {
                graph_version.update(|v| *v += 1);
            });

        let _ = source.add_event_listener_with_callback(
            "graph_updated",
            bump_updated.as_ref().unchecked_ref(),
        );
        let _ = source
            .add_event_listener_with_callback("graph_stale", bump_stale.as_ref().unchecked_ref());

        // Hand the closures to the JS side; they must outlive the component.
        // The EventSource itself is closed on cleanup so the callbacks can be
        // dropped safely with the component.
        bump_updated.forget();
        bump_stale.forget();

        on_cleanup(move || {
            source.close();
        });
    }

    #[cfg(not(feature = "hydrate"))]
    {
        let _ = graph_version;
    }
}
