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

        use gloo_timers::callback::Timeout;
        use std::{
            cell::{Cell, RefCell},
            rc::Rc,
        };
        use wasm_bindgen::JsCast;
        use wasm_bindgen::closure::Closure;
        use web_sys::{Event, EventSource, MessageEvent};

        #[derive(Default)]
        struct LiveSyncRuntime {
            source: Option<EventSource>,
            reconnect_timer: Option<Timeout>,
            on_open: Option<Closure<dyn FnMut(Event)>>,
            on_updated: Option<Closure<dyn FnMut(MessageEvent)>>,
            on_failed: Option<Closure<dyn FnMut(MessageEvent)>>,
            on_error: Option<Closure<dyn FnMut(Event)>>,
        }

        type ConnectHandle = Rc<RefCell<Option<Box<dyn Fn()>>>>;

        fn reconnect_delay_ms(attempt: u32) -> u32 {
            match attempt {
                0 | 1 => 1_000,
                2 => 2_500,
                3 => 5_000,
                _ => 10_000,
            }
        }

        fn stale_message_for_disconnect(delay_ms: u32) -> String {
            format!(
                "Live sync connection lost. Retrying in {:.1}s while showing the last known snapshot.",
                delay_ms as f32 / 1_000.0
            )
        }

        let runtime = Rc::new(RefCell::new(LiveSyncRuntime::default()));
        let attempts = Rc::new(Cell::new(0u32));
        let connect: ConnectHandle = Rc::new(RefCell::new(None));

        let schedule_reconnect = {
            let runtime = runtime.clone();
            let attempts = attempts.clone();
            let connect = connect.clone();
            move || {
                let attempt = attempts.get().saturating_add(1);
                attempts.set(attempt);
                let delay_ms = reconnect_delay_ms(attempt);
                sync_status.set(SyncStatus::Stale {
                    message: Some(stale_message_for_disconnect(delay_ms)),
                });

                let mut state = runtime.borrow_mut();
                if state.reconnect_timer.is_some() {
                    return;
                }
                if let Some(source) = state.source.take() {
                    source.close();
                }
                state.on_open = None;
                state.on_updated = None;
                state.on_failed = None;
                state.on_error = None;

                let runtime = runtime.clone();
                let connect = connect.clone();
                state.reconnect_timer = Some(Timeout::new(delay_ms, move || {
                    runtime.borrow_mut().reconnect_timer = None;
                    if let Some(connect) = connect.borrow().as_ref() {
                        connect();
                    }
                }));
            }
        };

        {
            let runtime = runtime.clone();
            let attempts = attempts.clone();
            let schedule_reconnect = schedule_reconnect.clone();
            *connect.borrow_mut() = Some(Box::new(move || {
                let Ok(source) = EventSource::new("/sse/events") else {
                    schedule_reconnect();
                    return;
                };

                let attempts_for_open = attempts.clone();
                let on_open: Closure<dyn FnMut(Event)> = Closure::new(move |_event: Event| {
                    attempts_for_open.set(0);
                    sync_status.set(SyncStatus::Fresh);
                });
                let on_updated: Closure<dyn FnMut(MessageEvent)> =
                    Closure::new(move |_event: MessageEvent| {
                        graph_version.update(|v| *v += 1);
                        sync_status.set(SyncStatus::Fresh);
                    });
                let on_failed: Closure<dyn FnMut(MessageEvent)> =
                    Closure::new(move |event: MessageEvent| {
                        let message = event
                            .data()
                            .as_string()
                            .and_then(|raw| serde_json::from_str::<SyncFailedPayload>(&raw).ok())
                            .and_then(|payload| payload.message);
                        graph_version.update(|v| *v += 1);
                        sync_status.set(SyncStatus::Stale { message });
                    });
                let on_error: Closure<dyn FnMut(Event)> = {
                    let schedule_reconnect = schedule_reconnect.clone();
                    Closure::new(move |_event: Event| {
                        schedule_reconnect();
                    })
                };

                let _ = source
                    .add_event_listener_with_callback("open", on_open.as_ref().unchecked_ref());
                let _ = source.add_event_listener_with_callback(
                    "graph_updated",
                    on_updated.as_ref().unchecked_ref(),
                );
                let _ = source.add_event_listener_with_callback(
                    "sync_failed",
                    on_failed.as_ref().unchecked_ref(),
                );
                let _ = source
                    .add_event_listener_with_callback("error", on_error.as_ref().unchecked_ref());

                let mut state = runtime.borrow_mut();
                state.source = Some(source);
                state.on_open = Some(on_open);
                state.on_updated = Some(on_updated);
                state.on_failed = Some(on_failed);
                state.on_error = Some(on_error);
            }));
        }

        if let Some(connect) = connect.borrow().as_ref() {
            connect();
        }

        // Anchor the runtime in the Leptos reactive tree so it lives for the
        // full lifetime of the component rather than being dropped at the end
        // of this function call. `Rc` is not Send+Sync so we use `new_local`.
        let _keep_alive = StoredValue::new_local(runtime);
    }

    #[cfg(not(feature = "hydrate"))]
    {
        let _ = (graph_version, sync_status);
    }
}

/// Global "Stale Data" banner. Renders only when `sync_status` is stale, so it
/// is safe to mount unconditionally above the routes.
#[component]
pub fn SyncStatusBanner(sync_status: RwSignal<SyncStatus>) -> impl IntoView {
    view! {
        <Show when=move || sync_status.get().is_stale()>
            <div class="mx-6 mt-3 rounded-md border border-amber-400/40 bg-amber-500/10 px-4 py-3 text-xs text-amber-100">
                <div class="flex items-start gap-3">
                    <div class="mt-1 h-2 w-2 rounded-full bg-amber-300"></div>
                    <div class="flex flex-col gap-1">
                        <span class="font-semibold uppercase tracking-[0.2em] text-amber-200">
                            "Stale Data"
                        </span>
                        <span class="text-amber-100/80">
                            {move || match sync_status.get() {
                                SyncStatus::Fresh => String::new(),
                                SyncStatus::Stale { message: Some(message) } => message,
                                SyncStatus::Stale { message: None } => {
                                    "A background sync reported stale data. The UI is showing the last successful snapshot until the next successful refresh.".to_string()
                                }
                            }}
                        </span>
                    </div>
                </div>
            </div>
        </Show>
    }
}
