use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

use brain_domain::{TargetRef, decode_path_segment, encode_path_segment};

use crate::api::{PrSummary, list_open_prs, resolve_legacy_target};

/// Read-only list of open pull requests for the active target. First step of
/// the PR-visibility trajectory (link -> view list -> merge); intentionally
/// no merge/close actions yet.
#[component]
pub fn PullRequestsPage() -> impl IntoView {
    let params = use_params_map();
    let org = Memo::new(move |_| params.with(|p| p.get("org").unwrap_or_default().to_string()));
    let repo = Memo::new(move |_| params.with(|p| p.get("repo").unwrap_or_default().to_string()));
    let branch =
        Memo::new(move |_| params.with(|p| p.get("branch").unwrap_or_default().to_string()));

    let data = Resource::new_blocking(
        move || (org.get(), repo.get(), branch.get()),
        |(o, r, b)| async move {
            let target = if b.is_empty() {
                resolve_legacy_target(o, r).await?
            } else {
                TargetRef::new(o, r, decode_path_segment(&b))
            };
            let prs = list_open_prs(target.clone()).await?;
            Ok::<_, crate::api::ApiError>(prs)
        },
    );

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
                    "Brain · Pull Requests"
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
                                        {prs.into_iter().map(pr_row).collect_view()}
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

fn pr_row(pr: PrSummary) -> impl IntoView {
    // created_at is ISO-8601; show the date portion without pulling a date dep.
    let created = pr.created_at.chars().take(10).collect::<String>();
    view! {
        <a
            href=pr.url
            target="_blank"
            rel="external noopener noreferrer"
            class="flex items-center gap-3 rounded-md border border-slate-800 bg-slate-900/40 px-4 py-3 hover:border-slate-700 hover:bg-slate-900/70 transition-colors"
        >
            <span class="shrink-0 font-mono text-xs text-slate-500">{format!("#{}", pr.number)}</span>
            <span class="min-w-0 flex-1 truncate text-sm text-slate-200">{pr.title}</span>
            {pr.draft.then(|| view! {
                <span class="shrink-0 rounded-full border border-slate-600 bg-slate-700/40 px-2 py-0.5 text-[10px] uppercase tracking-widest text-slate-400">
                    "draft"
                </span>
            })}
            <span class="shrink-0 text-xs text-slate-500">{pr.author}</span>
            <span class="shrink-0 font-mono text-[11px] text-slate-600">{created}</span>
        </a>
    }
}
