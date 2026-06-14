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

use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

use gitnodes_domain::{TargetRef, decode_path_segment, encode_path_segment};

use crate::api::{
    PrSummary, get_write_capabilities, list_open_prs, merge_pull_request, resolve_legacy_target,
};

/// Open pull requests for the active target, with an opt-in merge action for
/// push-capable users. Second/third step of the PR-visibility trajectory
/// (link -> view list -> merge). Merge is squash-only here; method choice,
/// close, and per-PR mergeable/checks preview are deferred.
#[component]
pub fn PullRequestsPage() -> impl IntoView {
    let params = use_params_map();
    let org = Memo::new(move |_| params.with(|p| p.get("org").unwrap_or_default().to_string()));
    let repo = Memo::new(move |_| params.with(|p| p.get("repo").unwrap_or_default().to_string()));
    let branch =
        Memo::new(move |_| params.with(|p| p.get("branch").unwrap_or_default().to_string()));

    // Bumped after a successful merge so the list drops the merged PR.
    let reload_tick = RwSignal::new(0u32);
    let data = Resource::new_blocking(
        move || (org.get(), repo.get(), branch.get(), reload_tick.get()),
        |(o, r, b, _)| async move {
            let target = if b.is_empty() {
                resolve_legacy_target(o, r).await?
            } else {
                TargetRef::new(o, r, decode_path_segment(&b))
            };
            let prs = list_open_prs(target.clone()).await?;
            Ok::<_, crate::api::ApiError>(prs)
        },
    );

    // Only push-capable users get the merge control.
    let capabilities = Resource::new(
        move || (org.get(), repo.get(), branch.get()),
        |(o, r, b)| async move {
            let target = if b.is_empty() {
                match resolve_legacy_target(o, r).await {
                    Ok(t) => t,
                    Err(_) => return None,
                }
            } else {
                TargetRef::new(o, r, decode_path_segment(&b))
            };
            get_write_capabilities(target).await.ok()
        },
    );
    let can_merge = Memo::new(move |_| {
        capabilities
            .get()
            .flatten()
            .map(|c| c.can_write_default_branch)
            .unwrap_or(false)
    });

    // PR number currently armed for confirmation (two-click guard before the
    // irreversible merge).
    let armed = RwSignal::new(Option::<u64>::None);
    let merge_action = Action::new(move |number: &u64| {
        let n = *number;
        let (o, r, b) = (
            org.get_untracked(),
            repo.get_untracked(),
            branch.get_untracked(),
        );
        async move {
            let target = if b.is_empty() {
                resolve_legacy_target(o, r).await?
            } else {
                TargetRef::new(o, r, decode_path_segment(&b))
            };
            merge_pull_request(target, n).await
        }
    });
    Effect::new(move |_| {
        if matches!(merge_action.value().get(), Some(Ok(_))) {
            armed.set(None);
            reload_tick.update(|t| *t += 1);
        }
    });

    let knowledge_href = move || {
        let (o, r, b) = (org.get(), repo.get(), branch.get());
        if b.is_empty() {
            format!("/{o}/{r}/knowledge")
        } else {
            format!("/{o}/{r}/{}/knowledge", encode_path_segment(&b))
        }
    };

    view! {
        <div class="min-h-screen bg-slate-950 text-slate-100">
            <header class="px-6 py-4 border-b border-slate-800 flex items-center gap-3">
                <div class="w-2 h-2 rounded-full bg-teal-400"></div>
                <h1 class="text-sm font-semibold tracking-wide uppercase text-slate-300">
                    "GitNodes · Pull Requests"
                </h1>
                <a
                    href=knowledge_href
                    rel="external"
                    class="ml-auto rounded-md border border-slate-800 px-2.5 py-1 text-xs text-slate-400 hover:border-slate-700 hover:text-slate-200"
                >
                    "← Knowledge"
                </a>
            </header>
            <main class="px-6 py-6 max-w-3xl mx-auto">
                {move || {
                    merge_action
                        .value()
                        .get()
                        .and_then(|r| r.err())
                        .map(|e| view! {
                            <div class="mb-3 rounded-md border border-rose-400/30 bg-rose-500/10 px-4 py-2 text-[11px] text-rose-200">
                                {e.actionable_message()}
                            </div>
                        })
                }}
                <Suspense fallback=|| view! {
                    <p class="text-sm text-slate-500">"Loading pull requests…"</p>
                }>
                    {move || match data.get() {
                        Some(Ok(prs)) => {
                            let count = prs.len();
                            if count == 0 {
                                view! {
                                    <p class="text-sm text-slate-500">"No open pull requests."</p>
                                }.into_any()
                            } else {
                                view! {
                                    <p class="mb-3 text-xs uppercase tracking-widest text-slate-500">
                                        {format!(
                                            "{count} open pull request{}",
                                            if count == 1 { "" } else { "s" },
                                        )}
                                    </p>
                                    <div class="space-y-2">
                                        {prs
                                            .into_iter()
                                            .map(|pr| pr_row(pr, armed, merge_action, can_merge))
                                            .collect_view()}
                                    </div>
                                }.into_any()
                            }
                        }
                        Some(Err(e)) => view! {
                            <div class="max-w-lg rounded-md border border-rose-400/30 bg-rose-500/10 px-5 py-4 text-rose-100">
                                <div class="text-xs font-semibold uppercase tracking-widest text-rose-200">
                                    "Pull requests unavailable"
                                </div>
                                <p class="mt-2 text-slate-200">
                                    "Could not load pull requests. Check access or branch state and retry."
                                </p>
                                <p class="mt-3 font-mono text-xs text-rose-200/90 break-words">{format!("{e}")}</p>
                            </div>
                        }.into_any(),
                        None => view! {
                            <p class="text-sm text-slate-500">"Loading pull requests…"</p>
                        }.into_any(),
                    }}
                </Suspense>
            </main>
        </div>
    }
}

type MergeAction = Action<u64, Result<crate::api::MergePrResult, crate::api::ApiError>>;

fn pr_row(
    pr: PrSummary,
    armed: RwSignal<Option<u64>>,
    merge_action: MergeAction,
    can_merge: Memo<bool>,
) -> impl IntoView {
    let n = pr.number;
    let url = pr.url;
    let title = pr.title;
    let author = pr.author;
    let draft = pr.draft;
    // created_at is ISO-8601; show the date portion without pulling a date dep.
    let created = pr.created_at.chars().take(10).collect::<String>();

    view! {
        <div class="flex items-center gap-3 rounded-md border border-slate-800 bg-slate-900/40 px-4 py-3">
            <a
                href=url
                target="_blank"
                rel="external noopener noreferrer"
                class="flex min-w-0 flex-1 items-center gap-3 hover:opacity-80 transition-opacity"
            >
                <span class="shrink-0 font-mono text-xs text-slate-500">{format!("#{n}")}</span>
                <span class="min-w-0 flex-1 truncate text-sm text-slate-200">{title}</span>
                {draft.then(|| view! {
                    <span class="shrink-0 rounded-full border border-slate-600 bg-slate-700/40 px-2 py-0.5 text-[10px] uppercase tracking-widest text-slate-400">
                        "draft"
                    </span>
                })}
                <span class="shrink-0 text-xs text-slate-500">{author}</span>
                <span class="shrink-0 font-mono text-[11px] text-slate-600">{created}</span>
            </a>
            <Show when=move || can_merge.get()>
                {move || {
                    let pending = merge_action.pending().get();
                    if pending && armed.get() == Some(n) {
                        view! {
                            <span class="shrink-0 px-2.5 py-1 text-xs text-slate-400">"Merging…"</span>
                        }.into_any()
                    } else if armed.get() == Some(n) {
                        view! {
                            <div class="flex shrink-0 gap-1">
                                <button
                                    on:click=move |_| { merge_action.dispatch(n); }
                                    disabled=move || merge_action.pending().get()
                                    class="rounded-md bg-emerald-500 px-2.5 py-1 text-xs font-semibold text-slate-950 hover:bg-emerald-400 disabled:opacity-50"
                                >
                                    "Confirm merge"
                                </button>
                                <button
                                    on:click=move |_| armed.set(None)
                                    class="rounded-md border border-slate-700 px-2.5 py-1 text-xs text-slate-400 hover:text-slate-200"
                                >
                                    "Cancel"
                                </button>
                            </div>
                        }.into_any()
                    } else {
                        view! {
                            <button
                                on:click=move |_| armed.set(Some(n))
                                disabled=move || merge_action.pending().get()
                                class="shrink-0 rounded-md border border-emerald-500/40 px-2.5 py-1 text-xs text-emerald-300 hover:bg-emerald-500/10 disabled:opacity-50"
                            >
                                "Merge"
                            </button>
                        }.into_any()
                    }
                }}
            </Show>
        </div>
    }
}
