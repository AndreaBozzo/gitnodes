use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

use crate::api::{
    AuditEntry, PendingSyncEntry, SessionEntry, list_pending_sync, list_sessions, load_audit_log,
    revoke_session,
};

pub mod views;
pub use views::ViewsAdminPage;

#[component]
pub fn AdminPage() -> impl IntoView {
    let params = use_params_map();
    let target_prefix = Memo::new(move |_| {
        let (org, repo, branch) = params.with(|p| {
            (
                p.get("org").unwrap_or_default().to_string(),
                p.get("repo").unwrap_or_default().to_string(),
                p.get("branch").unwrap_or_default().to_string(),
            )
        });
        if org.is_empty() || repo.is_empty() {
            String::new()
        } else if branch.is_empty() {
            format!("/{org}/{repo}")
        } else {
            format!("/{org}/{repo}/{branch}")
        }
    });
    let kind_filter = RwSignal::new(String::new());
    let reload_tick = RwSignal::new(0u32);

    let audit = Resource::new_blocking(
        move || (kind_filter.get(), reload_tick.get()),
        |(kind, _)| async move {
            let k = if kind.is_empty() { None } else { Some(kind) };
            load_audit_log(k, Some(200)).await
        },
    );

    let sessions = Resource::new_blocking(
        move || reload_tick.get(),
        |_| async move { list_sessions().await },
    );

    let pending_sync = Resource::new_blocking(
        move || reload_tick.get(),
        |_| async move { list_pending_sync().await },
    );

    let revoke = Action::new(move |id: &String| {
        let id = id.clone();
        async move {
            let _ = revoke_session(id).await;
        }
    });

    Effect::new(move |_| {
        if revoke.version().get() > 0 {
            reload_tick.update(|t| *t += 1);
        }
    });

    view! {
        <div class="min-h-screen bg-slate-950 text-slate-100">
            <header class="px-6 py-4 border-b border-slate-800 flex items-center gap-3">
                <div class="w-2 h-2 rounded-full bg-amber-400"></div>
                <h1 class="text-sm font-semibold tracking-wide uppercase text-slate-300">
                    "Brain · Admin"
                </h1>
                <span class="text-xs text-slate-500 ml-2">
                    {move || format!("{}/admin", target_prefix.get())}
                </span>
                <a
                    href=move || format!("{}/admin/views", target_prefix.get())
                    rel="external"
                    class="ml-auto text-xs text-teal-300 hover:text-teal-200"
                >
                    "edit views →"
                </a>
                <a
                    href=move || format!("{}/knowledge", target_prefix.get())
                    rel="external"
                    class="text-xs text-slate-400 hover:text-slate-200"
                >
                    "← back to knowledge"
                </a>
            </header>

            <main class="p-6 space-y-8 max-w-6xl mx-auto">
                <section>
                    <div class="flex items-center gap-3 mb-3">
                        <h2 class="text-xs uppercase tracking-widest text-slate-400">
                            "Audit log"
                        </h2>
                        <select
                            class="ml-auto bg-slate-900 border border-slate-800 rounded px-2 py-1 text-xs text-slate-200"
                            on:change=move |ev| {
                                kind_filter.set(event_target_value(&ev));
                            }
                            prop:value=move || kind_filter.get()
                        >
                            <option value="">"all kinds"</option>
                            <option value="login_ok">"login_ok"</option>
                            <option value="login_fail">"login_fail"</option>
                            <option value="logout">"logout"</option>
                            <option value="create">"create"</option>
                            <option value="update">"update"</option>
                            <option value="delete">"delete"</option>
                            <option value="create_folder">"create_folder"</option>
                            <option value="revoke_session">"revoke_session"</option>
                            <option value="api_error">"api_error"</option>
                        </select>
                        <button
                            class="px-2 py-1 rounded bg-slate-800 hover:bg-slate-700 text-xs"
                            on:click=move |_| reload_tick.update(|t| *t += 1)
                        >
                            "refresh"
                        </button>
                    </div>
                    <Suspense fallback=|| view! { <p class="text-xs text-slate-500">"loading…"</p> }>
                        {move || audit.get().map(|res| match res {
                            Ok(rows) => AuditTable(AuditTableProps { rows }).into_any(),
                            Err(e) => view! {
                                <p class="text-xs text-rose-300">{format!("failed: {e}")}</p>
                            }.into_any(),
                        })}
                    </Suspense>
                </section>

                <section>
                    <div class="flex items-center gap-3 mb-3">
                        <h2 class="text-xs uppercase tracking-widest text-slate-400">
                            "Active sessions"
                        </h2>
                    </div>
                    <Suspense fallback=|| view! { <p class="text-xs text-slate-500">"loading…"</p> }>
                        {move || sessions.get().map(|res| match res {
                            Ok(rows) => SessionTable(SessionTableProps {
                                rows,
                                on_revoke: Callback::new(move |id: String| { revoke.dispatch(id); }),
                            }).into_any(),
                            Err(e) => view! {
                                <p class="text-xs text-rose-300">{format!("failed: {e}")}</p>
                            }.into_any(),
                        })}
                    </Suspense>
                </section>

                <section>
                    <div class="flex items-center gap-3 mb-3">
                        <h2 class="text-xs uppercase tracking-widest text-slate-400">
                            "Pending provider sync"
                        </h2>
                        <span class="text-xs text-slate-500">
                            "work item changes saved in Brain but not yet propagated to the forge"
                        </span>
                    </div>
                    <Suspense fallback=|| view! { <p class="text-xs text-slate-500">"loading…"</p> }>
                        {move || pending_sync.get().map(|res| match res {
                            Ok(rows) => PendingSyncTable(PendingSyncTableProps { rows }).into_any(),
                            Err(e) => view! {
                                <p class="text-xs text-rose-300">{format!("failed: {e}")}</p>
                            }.into_any(),
                        })}
                    </Suspense>
                </section>
            </main>
        </div>
    }
}

#[component]
fn AuditTable(rows: Vec<AuditEntry>) -> impl IntoView {
    if rows.is_empty() {
        return view! {
            <p class="text-xs text-slate-500 italic">"no events yet"</p>
        }
        .into_any();
    }
    view! {
        <div class="border border-slate-800 rounded-md overflow-hidden">
            <table class="w-full text-xs">
                <thead class="bg-slate-900 text-slate-400 uppercase tracking-widest">
                    <tr>
                        <th class="text-left px-3 py-2 w-40">"timestamp"</th>
                        <th class="text-left px-3 py-2 w-32">"kind"</th>
                        <th class="text-left px-3 py-2 w-32">"actor"</th>
                        <th class="text-left px-3 py-2">"detail"</th>
                    </tr>
                </thead>
                <tbody>
                    {rows.into_iter().map(|r| {
                        let kind_class = match r.kind.as_str() {
                            "login_fail" | "api_error" => "text-rose-300",
                            "login_ok" => "text-emerald-300",
                            "delete" | "revoke_session" => "text-amber-300",
                            _ => "text-slate-200",
                        };
                        view! {
                            <tr class="border-t border-slate-800 hover:bg-slate-900/50">
                                <td class="px-3 py-1.5 text-slate-400 font-mono">{r.ts}</td>
                                <td class=format!("px-3 py-1.5 font-mono {}", kind_class)>{r.kind}</td>
                                <td class="px-3 py-1.5 text-slate-300">{r.actor.unwrap_or_else(|| "—".to_string())}</td>
                                <td class="px-3 py-1.5 text-slate-400 font-mono truncate">{r.detail.unwrap_or_default()}</td>
                            </tr>
                        }
                    }).collect_view()}
                </tbody>
            </table>
        </div>
    }.into_any()
}

#[component]
fn PendingSyncTable(rows: Vec<PendingSyncEntry>) -> impl IntoView {
    if rows.is_empty() {
        return view! {
            <p class="text-xs text-slate-500 italic">"nothing pending — all changes propagated"</p>
        }
        .into_any();
    }
    view! {
        <div class="border border-slate-800 rounded-md overflow-hidden">
            <table class="w-full text-xs">
                <thead class="bg-slate-900 text-slate-400 uppercase tracking-widest">
                    <tr>
                        <th class="text-left px-3 py-2 w-48">"target"</th>
                        <th class="text-left px-3 py-2">"work item"</th>
                        <th class="text-left px-3 py-2 w-24">"kind"</th>
                        <th class="text-left px-3 py-2 w-20">"attempts"</th>
                        <th class="text-left px-3 py-2 w-40">"last attempt"</th>
                        <th class="text-left px-3 py-2">"last error"</th>
                    </tr>
                </thead>
                <tbody>
                    {rows.into_iter().map(|r| {
                        let target = format!("{}/{} ({})", r.org, r.repo, r.branch);
                        view! {
                            <tr class="border-t border-slate-800 hover:bg-slate-900/50">
                                <td class="px-3 py-1.5 text-slate-300 font-mono">{target}</td>
                                <td class="px-3 py-1.5 text-slate-200 font-mono truncate">{r.brain_id}</td>
                                <td class="px-3 py-1.5 text-slate-400 font-mono">{r.kind}</td>
                                <td class="px-3 py-1.5 text-amber-300 font-mono">{r.attempts}</td>
                                <td class="px-3 py-1.5 text-slate-400 font-mono">{r.last_attempt_at}</td>
                                <td class="px-3 py-1.5 text-rose-300 font-mono truncate">{r.last_error.unwrap_or_default()}</td>
                            </tr>
                        }
                    }).collect_view()}
                </tbody>
            </table>
        </div>
    }.into_any()
}

#[component]
fn SessionTable(rows: Vec<SessionEntry>, on_revoke: Callback<String>) -> impl IntoView {
    if rows.is_empty() {
        return view! {
            <p class="text-xs text-slate-500 italic">"no active sessions"</p>
        }
        .into_any();
    }
    view! {
        <div class="border border-slate-800 rounded-md overflow-hidden">
            <table class="w-full text-xs">
                <thead class="bg-slate-900 text-slate-400 uppercase tracking-widest">
                    <tr>
                        <th class="text-left px-3 py-2">"session id"</th>
                        <th class="text-left px-3 py-2 w-48">"expires"</th>
                        <th class="text-right px-3 py-2 w-24"></th>
                    </tr>
                </thead>
                <tbody>
                    {rows.into_iter().map(|r| {
                        let id = r.id.clone();
                        let short = if id.len() > 16 { format!("{}…", &id[..16]) } else { id.clone() };
                        view! {
                            <tr class="border-t border-slate-800 hover:bg-slate-900/50">
                                <td class="px-3 py-1.5 text-slate-300 font-mono">{short}</td>
                                <td class="px-3 py-1.5 text-slate-400 font-mono">{r.expiry_date}</td>
                                <td class="px-3 py-1.5 text-right">
                                    <button
                                        class="px-2 py-0.5 rounded bg-rose-500/20 border border-rose-400/40 text-rose-200 hover:bg-rose-500/30"
                                        on:click=move |_| on_revoke.run(id.clone())
                                    >
                                        "revoke"
                                    </button>
                                </td>
                            </tr>
                        }
                    }).collect_view()}
                </tbody>
            </table>
        </div>
    }.into_any()
}
