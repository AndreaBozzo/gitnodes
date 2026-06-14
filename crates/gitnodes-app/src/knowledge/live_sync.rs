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

#[cfg(feature = "hydrate")]
use gitnodes_domain::decode_path_segment;
use leptos::prelude::*;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SyncStatus {
    #[default]
    Fresh,
    Reconnecting {
        message: Option<String>,
    },
    Degraded {
        message: Option<String>,
    },
}

impl SyncStatus {
    pub fn is_reconnecting(&self) -> bool {
        matches!(self, Self::Reconnecting { .. })
    }

    pub fn is_degraded(&self) -> bool {
        matches!(self, Self::Degraded { .. })
    }

    pub fn message_or(&self, fallback: &'static str) -> String {
        match self {
            Self::Reconnecting {
                message: Some(message),
            }
            | Self::Degraded {
                message: Some(message),
            } => message.clone(),
            _ => fallback.to_string(),
        }
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
            target: gitnodes_domain::TargetRef,
            message: Option<String>,
        }

        #[derive(serde::Deserialize)]
        struct TargetPayload {
            target: gitnodes_domain::TargetRef,
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
            startup_timer: Option<Timeout>,
            reconnect_timer: Option<Timeout>,
            reconnect_notice_timer: Option<Timeout>,
            on_open: Option<Closure<dyn FnMut(Event)>>,
            on_updated: Option<Closure<dyn FnMut(MessageEvent)>>,
            on_failed: Option<Closure<dyn FnMut(MessageEvent)>>,
            on_work_item: Option<Closure<dyn FnMut(MessageEvent)>>,
            on_binding: Option<Closure<dyn FnMut(MessageEvent)>>,
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

        fn reconnecting_message_for_disconnect(delay_ms: u32) -> String {
            format!(
                "Showing the last snapshot. Retrying live sync in {:.1}s.",
                delay_ms as f32 / 1_000.0
            )
        }

        fn is_workspace_path(path: &str) -> bool {
            if path == "/knowledge"
                || path.starts_with("/knowledge/")
                || path == "/admin"
                || path.starts_with("/admin/")
            {
                return true;
            }

            let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
            match parts.as_slice() {
                [org, repo, "knowledge", ..] | [org, repo, "admin", ..] => {
                    !org.is_empty() && !repo.is_empty()
                }
                [org, repo, branch, "knowledge", ..] | [org, repo, branch, "admin", ..] => {
                    !org.is_empty() && !repo.is_empty() && !branch.is_empty()
                }
                _ => false,
            }
        }

        fn live_sync_enabled_for_location() -> bool {
            web_sys::window()
                .and_then(|window| window.location().pathname().ok())
                .is_some_and(|path| is_workspace_path(&path))
        }

        fn active_target_from_location() -> Option<gitnodes_domain::TargetRef> {
            let path = web_sys::window()?.location().pathname().ok()?;
            let parts: Vec<&str> = path.trim_start_matches('/').splitn(5, '/').collect();
            match parts.as_slice() {
                [org, repo, branch, "knowledge"] | [org, repo, branch, "admin"]
                    if !org.is_empty() && !repo.is_empty() && !branch.is_empty() =>
                {
                    Some(gitnodes_domain::TargetRef::new(
                        *org,
                        *repo,
                        decode_path_segment(branch),
                    ))
                }
                [org, repo, branch, "knowledge", _] | [org, repo, branch, "admin", _]
                    if !org.is_empty() && !repo.is_empty() && !branch.is_empty() =>
                {
                    Some(gitnodes_domain::TargetRef::new(
                        *org,
                        *repo,
                        decode_path_segment(branch),
                    ))
                }
                _ => None,
            }
        }

        fn event_matches_active_target(raw: &str) -> bool {
            let Some(active) = active_target_from_location() else {
                return true;
            };
            serde_json::from_str::<TargetPayload>(raw)
                .map(|payload| payload.target == active)
                .unwrap_or(false)
        }

        let runtime = Rc::new(RefCell::new(LiveSyncRuntime::default()));
        let attempts = Rc::new(Cell::new(0u32));
        let connect: ConnectHandle = Rc::new(RefCell::new(None));

        let schedule_reconnect = {
            let runtime = runtime.clone();
            let attempts = attempts.clone();
            let connect = connect.clone();
            move || {
                if !live_sync_enabled_for_location() {
                    runtime.borrow_mut().reconnect_notice_timer = None;
                    sync_status.set(SyncStatus::Fresh);
                    return;
                }

                let attempt = attempts.get().saturating_add(1);
                attempts.set(attempt);
                let delay_ms = reconnect_delay_ms(attempt);

                let mut state = runtime.borrow_mut();
                if state.reconnect_timer.is_some() {
                    return;
                }
                if let Some(source) = state.source.take() {
                    source.close();
                }
                state.startup_timer = None;
                state.on_open = None;
                state.on_updated = None;
                state.on_failed = None;
                state.on_work_item = None;
                state.on_binding = None;
                state.on_error = None;

                if sync_status.with_untracked(SyncStatus::is_reconnecting) {
                    sync_status.set(SyncStatus::Reconnecting {
                        message: Some(reconnecting_message_for_disconnect(delay_ms)),
                    });
                } else if !sync_status.with_untracked(SyncStatus::is_degraded)
                    && state.reconnect_notice_timer.is_none()
                {
                    let runtime_for_notice = runtime.clone();
                    let attempts_for_notice = attempts.clone();
                    state.reconnect_notice_timer = Some(Timeout::new(4_000, move || {
                        runtime_for_notice.borrow_mut().reconnect_notice_timer = None;
                        if !live_sync_enabled_for_location()
                            || attempts_for_notice.get() == 0
                            || sync_status.with_untracked(SyncStatus::is_degraded)
                        {
                            return;
                        }
                        let delay_ms = reconnect_delay_ms(attempts_for_notice.get());
                        sync_status.set(SyncStatus::Reconnecting {
                            message: Some(reconnecting_message_for_disconnect(delay_ms)),
                        });
                    }));
                }

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
                if !live_sync_enabled_for_location() {
                    sync_status.set(SyncStatus::Fresh);
                    return;
                }

                // Slice δ: scope the stream to the active target server-side so
                // a noisy target can't starve others. The server falls back to
                // the env-default target when no param is sent (legacy deploy).
                let sse_url = match active_target_from_location() {
                    Some(t) => {
                        let key = format!("{}/{}/{}", t.org, t.repo, t.branch);
                        format!("/sse/events?target={}", js_sys::encode_uri_component(&key))
                    }
                    None => "/sse/events".to_string(),
                };
                let Ok(source) = EventSource::new(&sse_url) else {
                    schedule_reconnect();
                    return;
                };

                let attempts_for_open = attempts.clone();
                let runtime_for_open = runtime.clone();
                let on_open: Closure<dyn FnMut(Event)> = Closure::new(move |_event: Event| {
                    attempts_for_open.set(0);
                    runtime_for_open.borrow_mut().reconnect_notice_timer = None;
                    if !sync_status.with_untracked(SyncStatus::is_degraded) {
                        sync_status.set(SyncStatus::Fresh);
                    }
                });
                let on_updated: Closure<dyn FnMut(MessageEvent)> =
                    Closure::new(move |event: MessageEvent| {
                        let Some(raw) = event.data().as_string() else {
                            return;
                        };
                        if !event_matches_active_target(&raw) {
                            return;
                        }
                        graph_version.update(|v| *v += 1);
                        sync_status.set(SyncStatus::Fresh);
                    });
                let on_failed: Closure<dyn FnMut(MessageEvent)> =
                    Closure::new(move |event: MessageEvent| {
                        let Some(raw) = event.data().as_string() else {
                            return;
                        };
                        let Ok(payload) = serde_json::from_str::<SyncFailedPayload>(&raw) else {
                            return;
                        };
                        if active_target_from_location()
                            .is_some_and(|active| active != payload.target)
                        {
                            return;
                        }
                        let message = payload.message;
                        graph_version.update(|v| *v += 1);
                        sync_status.set(SyncStatus::Degraded { message });
                    });
                // Granular work item events: for the 3.2-α slice both variants
                // simply bump the version so existing Resources refetch. A
                // future iteration can demux on `brain_id` to refresh only the
                // affected detail panel without a full graph reload.
                let on_work_item: Closure<dyn FnMut(MessageEvent)> =
                    Closure::new(move |event: MessageEvent| {
                        let Some(raw) = event.data().as_string() else {
                            return;
                        };
                        if !event_matches_active_target(&raw) {
                            return;
                        }
                        graph_version.update(|v| *v += 1);
                        sync_status.set(SyncStatus::Fresh);
                    });
                let on_binding: Closure<dyn FnMut(MessageEvent)> =
                    Closure::new(move |event: MessageEvent| {
                        let Some(raw) = event.data().as_string() else {
                            return;
                        };
                        if !event_matches_active_target(&raw) {
                            return;
                        }
                        graph_version.update(|v| *v += 1);
                        sync_status.set(SyncStatus::Fresh);
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
                let _ = source.add_event_listener_with_callback(
                    "work_item_updated",
                    on_work_item.as_ref().unchecked_ref(),
                );
                let _ = source.add_event_listener_with_callback(
                    "binding_updated",
                    on_binding.as_ref().unchecked_ref(),
                );
                let _ = source
                    .add_event_listener_with_callback("error", on_error.as_ref().unchecked_ref());

                let mut state = runtime.borrow_mut();
                state.source = Some(source);
                state.on_open = Some(on_open);
                state.on_updated = Some(on_updated);
                state.on_failed = Some(on_failed);
                state.on_work_item = Some(on_work_item);
                state.on_binding = Some(on_binding);
                state.on_error = Some(on_error);
            }));
        }

        {
            let runtime = runtime.clone();
            let connect = connect.clone();
            let runtime_for_timer = runtime.clone();
            runtime.borrow_mut().startup_timer = Some(Timeout::new(0, move || {
                runtime_for_timer.borrow_mut().startup_timer = None;
                if let Some(connect) = connect.borrow().as_ref() {
                    connect();
                }
            }));
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

/// Global sync status. Reconnect churn stays quiet; real sync failures remain
/// visible because the user may need to refresh or inspect the provider.
#[component]
pub fn SyncStatusBanner(sync_status: RwSignal<SyncStatus>) -> impl IntoView {
    view! {
        <Show when=move || sync_status.get().is_reconnecting()>
            <div class="mx-6 mt-2 flex justify-end">
                <div class="inline-flex max-w-full items-center gap-2 rounded-full border border-slate-600/70 bg-slate-900/90 px-3 py-1.5 text-xs text-slate-300 shadow-sm">
                    <span class="h-2 w-2 rounded-full bg-sky-300"></span>
                    <span class="font-medium text-slate-200">"Reconnecting"</span>
                    <span class="hidden text-slate-400 sm:inline">
                        {move || {
                            sync_status
                                .get()
                                .message_or("Showing the last snapshot. Retrying live sync.")
                        }}
                    </span>
                </div>
            </div>
        </Show>

        <Show when=move || sync_status.get().is_degraded()>
            <div class="mx-6 mt-3 rounded-md border border-amber-400/35 bg-amber-500/10 px-4 py-3 text-xs text-amber-100">
                <div class="flex items-start gap-3">
                    <div class="mt-1 h-2 w-2 rounded-full bg-amber-300"></div>
                    <div class="flex flex-col gap-1">
                        <span class="font-semibold uppercase tracking-[0.12em] text-amber-200">
                            "Sync Needs Attention"
                        </span>
                        <span class="text-amber-100/80">
                            {move || {
                                sync_status.get().message_or(
                                    "A background sync failed. GitNodes is showing the last successful snapshot; use Refresh if the view looks out of date.",
                                )
                            }}
                        </span>
                    </div>
                </div>
            </div>
        </Show>
    }
}
