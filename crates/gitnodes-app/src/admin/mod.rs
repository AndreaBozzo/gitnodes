use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

use crate::api::{
    AuditEntry, PendingSyncEntry, ProjectionStatus, ProjectionStatusEntry, SessionEntry,
    get_projection_status, list_pending_sync, list_sessions, load_audit_log, revoke_session,
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

    let projection_status = Resource::new_blocking(
        move || reload_tick.get(),
        |_| async move { get_projection_status().await },
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
                        <h2 class="text-xs uppercase tracking-widest text-slate-300">
                            "Projection status"
                        </h2>
                        <span class="text-xs text-slate-500">
                            "schema, target readiness, counts, and rebuild cost"
                        </span>
                    </div>
                    <Suspense fallback=|| view! { <p class="text-xs text-slate-500">"loading…"</p> }>
                        {move || projection_status.get().map(|res| match res {
                            Ok(status) => ProjectionStatusPanel(ProjectionStatusPanelProps { status }).into_any(),
                            Err(e) => view! {
                                <p class="text-xs text-rose-300">{format!("failed: {e}")}</p>
                            }.into_any(),
                        })}
                    </Suspense>
                </section>

                <section>
                    <div class="flex items-center gap-3 mb-3">
                        <h2 class="text-xs uppercase tracking-widest text-slate-300">
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
                        <h2 class="text-xs uppercase tracking-widest text-slate-300">
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
                        <h2 class="text-xs uppercase tracking-widest text-slate-300">
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
        <div class="border border-slate-800 rounded-md overflow-x-auto">
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
        <div class="border border-slate-800 rounded-md overflow-x-auto">
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
        <div class="border border-slate-800 rounded-md overflow-x-auto">
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

#[component]
fn ProjectionStatusPanel(status: ProjectionStatus) -> impl IntoView {
    let ProjectionStatus {
        schema_version,
        targets,
        webhook_lag_seconds,
        rate_limit_remaining,
    } = status;
    let lag_label = webhook_lag_seconds
        .map(|n| format!("{n}s"))
        .unwrap_or_else(|| "—".to_string());
    let quota_label = rate_limit_remaining
        .map(|n| n.to_string())
        .unwrap_or_else(|| "—".to_string());
    let target_count = targets.len();
    let ready_count = targets.iter().filter(|r| r.status == "ready").count();
    let running_count = targets.iter().filter(|r| r.status == "running").count();
    let error_count = targets.iter().filter(|r| r.status == "error").count();
    let file_total: i64 = targets.iter().map(|r| r.file_count).sum();
    let node_total: i64 = targets.iter().map(|r| r.node_count).sum();
    let work_item_total: i64 = targets.iter().map(|r| r.work_item_count).sum();
    let (health_label, health_class) = if target_count == 0 {
        (
            "No targets",
            "rounded-full border border-slate-700 bg-slate-900 px-2.5 py-1 text-[10px] font-semibold uppercase tracking-widest text-slate-400",
        )
    } else if error_count > 0 {
        (
            "Needs attention",
            "rounded-full border border-rose-400/30 bg-rose-500/10 px-2.5 py-1 text-[10px] font-semibold uppercase tracking-widest text-rose-200",
        )
    } else if running_count > 0 {
        (
            "Rebuilding",
            "rounded-full border border-amber-400/30 bg-amber-500/10 px-2.5 py-1 text-[10px] font-semibold uppercase tracking-widest text-amber-100",
        )
    } else {
        (
            "Ready",
            "rounded-full border border-emerald-400/30 bg-emerald-500/10 px-2.5 py-1 text-[10px] font-semibold uppercase tracking-widest text-emerald-200",
        )
    };

    view! {
        <div class="space-y-4">
            <div class="grid gap-3 sm:grid-cols-3">
                <div class="rounded-md border border-slate-800 bg-slate-900/60 px-4 py-3">
                    <div class="flex items-center justify-between gap-3">
                        <span class="text-[10px] uppercase tracking-widest text-slate-500">"Projection"</span>
                        <span class=health_class>{health_label}</span>
                    </div>
                    <div class="mt-3 flex items-end gap-2">
                        <span class="text-2xl font-semibold tabular-nums text-slate-100">{ready_count}</span>
                        <span class="pb-1 text-xs text-slate-500">"/ "{target_count}" targets ready"</span>
                    </div>
                </div>
                <div class="rounded-md border border-slate-800 bg-slate-900/60 px-4 py-3">
                    <div class="text-[10px] uppercase tracking-widest text-slate-500">"Indexed content"</div>
                    <div class="mt-3 grid grid-cols-3 gap-3 text-xs">
                        <div>
                            <div class="text-lg font-semibold tabular-nums text-slate-100">{file_total}</div>
                            <div class="text-slate-500">"files"</div>
                        </div>
                        <div>
                            <div class="text-lg font-semibold tabular-nums text-slate-100">{node_total}</div>
                            <div class="text-slate-500">"nodes"</div>
                        </div>
                        <div>
                            <div class="text-lg font-semibold tabular-nums text-slate-100">{work_item_total}</div>
                            <div class="text-slate-500">"items"</div>
                        </div>
                    </div>
                </div>
                <div class="rounded-md border border-slate-800 bg-slate-900/60 px-4 py-3">
                    <div class="text-[10px] uppercase tracking-widest text-slate-500">"Operations"</div>
                    <div class="mt-3 grid grid-cols-3 gap-3 text-xs">
                        <div>
                            <div class="font-mono text-sm text-slate-100">"v"{schema_version}</div>
                            <div class="text-slate-500">"schema"</div>
                        </div>
                        <div>
                            <div class="font-mono text-sm text-slate-100">{lag_label}</div>
                            <div class="text-slate-500">"webhook"</div>
                        </div>
                        <div>
                            <div class="font-mono text-sm text-slate-100">{quota_label}</div>
                            <div class="text-slate-500">"rate"</div>
                        </div>
                    </div>
                </div>
            </div>
            {if targets.is_empty() {
                view! {
                    <div class="rounded-md border border-slate-800 bg-slate-900/40 px-4 py-3 text-xs text-slate-500">
                        "No targets registered yet."
                    </div>
                }.into_any()
            } else {
                view! {
                    <div class="border border-slate-800 rounded-md overflow-x-auto">
                        <table class="w-full text-xs">
                            <thead class="bg-slate-900 text-slate-400 uppercase tracking-widest">
                                <tr>
                                    <th class="text-left px-3 py-2">"target"</th>
                                    <th class="text-left px-3 py-2 w-20">"status"</th>
                                    <th class="text-right px-3 py-2 w-16">"files"</th>
                                    <th class="text-right px-3 py-2 w-16">"nodes"</th>
                                    <th class="text-right px-3 py-2 w-16">"edges"</th>
                                    <th class="text-right px-3 py-2 w-16">"items"</th>
                                    <th class="text-right px-3 py-2 w-24">"rebuild"</th>
                                    <th class="text-left px-3 py-2 w-40">"last success"</th>
                                    <th class="text-left px-3 py-2">"last error"</th>
                                </tr>
                            </thead>
                            <tbody>
                                {targets.into_iter().map(|r: ProjectionStatusEntry| {
                                    let target = format!("{}/{} ({})", r.org, r.repo, r.branch);
                                    let status_class = match r.status.as_str() {
                                        "ready" => "rounded-full border border-emerald-400/30 bg-emerald-500/10 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-widest text-emerald-200",
                                        "error" => "rounded-full border border-rose-400/30 bg-rose-500/10 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-widest text-rose-200",
                                        "running" => "rounded-full border border-amber-400/30 bg-amber-500/10 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-widest text-amber-100",
                                        _ => "rounded-full border border-slate-700 bg-slate-900 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-widest text-slate-400",
                                    };
                                    let duration = r.last_rebuild_duration_ms
                                        .map(|n| format!("{n} ms"))
                                        .unwrap_or_else(|| "—".to_string());
                                    let last_success = r.last_success_at.unwrap_or_else(|| "—".to_string());
                                    let last_error = r.last_error.unwrap_or_default();
                                    view! {
                                        <tr class="border-t border-slate-800 hover:bg-slate-900/50">
                                            <td class="px-3 py-1.5 text-slate-200 font-mono">{target}</td>
                                            <td class="px-3 py-1.5"><span class=status_class>{r.status}</span></td>
                                            <td class="px-3 py-1.5 text-right text-slate-300 font-mono">{r.file_count}</td>
                                            <td class="px-3 py-1.5 text-right text-slate-300 font-mono">{r.node_count}</td>
                                            <td class="px-3 py-1.5 text-right text-slate-300 font-mono">{r.edge_count}</td>
                                            <td class="px-3 py-1.5 text-right text-slate-300 font-mono">{r.work_item_count}</td>
                                            <td class="px-3 py-1.5 text-right text-slate-300 font-mono">{duration}</td>
                                            <td class="px-3 py-1.5 text-slate-400 font-mono">{last_success}</td>
                                            <td class="px-3 py-1.5 text-rose-300 font-mono truncate">{last_error}</td>
                                        </tr>
                                    }
                                }).collect_view()}
                            </tbody>
                        </table>
                    </div>
                }.into_any()
            }}
        </div>
    }
}
