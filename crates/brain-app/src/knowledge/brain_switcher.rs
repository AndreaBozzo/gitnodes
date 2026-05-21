//! Multi-tenant routing: `/{org}/{repo}/knowledge` and the Brain Switcher
//! sidebar component for discovering accessible repositories.
//!
//! `KnowledgePageForTarget` mirrors `KnowledgePage` but reads `org` and `repo`
//! from the route params and calls the target-explicit server fns instead of
//! the boot-env-bound ones. The rest of the knowledge UI (graph canvas, editor,
//! filter panel) is identical — only the data-loading layer changes.

use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

use crate::api::{
    AccessibleTargetState, FileQueryFilters, list_accessible_targets, list_brain_files,
    load_brain_config_status_for_target, load_brain_graph_for_target, resolve_legacy_target,
};
use crate::app::{GraphVersion, SyncStatusSignal};
use brain_domain::{TargetRef, decode_path_segment, encode_path_segment};

fn knowledge_loading_view() -> impl IntoView {
    view! {
        <div class="min-h-screen flex items-center justify-center bg-slate-950 text-slate-400 text-sm">
            "Loading knowledge graph…"
        </div>
    }
}

// ---------------------------------------------------------------------------
// KnowledgePageForTarget
// ---------------------------------------------------------------------------

/// Entry point for `/{org}/{repo}/knowledge`. Reads route params, builds the
/// explicit target key, and delegates to the knowledge view.
#[component]
pub fn KnowledgePageForTarget() -> impl IntoView {
    let params = use_params_map();

    let org = Memo::new(move |_| params.with(|p| p.get("org").unwrap_or_default().to_string()));
    let repo = Memo::new(move |_| params.with(|p| p.get("repo").unwrap_or_default().to_string()));
    let branch =
        Memo::new(move |_| params.with(|p| p.get("branch").unwrap_or_default().to_string()));

    let graph_version = expect_context::<GraphVersion>().0;
    let sync_status = expect_context::<SyncStatusSignal>().0;

    // Reload whenever graph_version bumps (webhook / manual refresh) or the
    // target changes (user switched to a different repo via Brain Switcher).
    let data = Resource::new_blocking(
        move || (graph_version.get(), org.get(), repo.get(), branch.get()),
        |(_, o, r, b)| async move {
            let target = if b.is_empty() {
                resolve_legacy_target(o, r).await?
            } else {
                TargetRef::new(o, r, decode_path_segment(&b))
            };
            let (nodes, edges) = load_brain_graph_for_target(target.clone()).await?;
            let config_status = load_brain_config_status_for_target(target.clone()).await?;
            let files = list_brain_files(FileQueryFilters {
                org: Some(target.org.clone()),
                repo: Some(target.repo.clone()),
                branch: Some(target.branch.clone()),
                ..Default::default()
            })
            .await?;
            Ok::<_, crate::api::ApiError>((target, nodes, edges, config_status, files))
        },
    );

    view! {
        <Suspense fallback=knowledge_loading_view>
            {move || {
                match data.get() {
                    Some(Ok((target, nodes, edges, config_status, files))) => {
                        use super::page::KnowledgeViewProps;
                        super::page::KnowledgeView(KnowledgeViewProps {
                            nodes,
                            edges,
                            files,
                            config: config_status.config,
                            config_diagnostic: config_status.diagnostic,
                            graph_version,
                            sync_status,
                            target_ref: target,
                        })
                        .into_any()
                    }
                    Some(Err(e)) => view! {
                        <div class="min-h-screen flex items-center justify-center bg-slate-950 text-rose-300 text-sm">
                            {format!("Failed to load graph/config for target: {e}")}
                        </div>
                    }.into_any(),
                    _ => knowledge_loading_view().into_any(),
                }
            }}
        </Suspense>
    }
}

// ---------------------------------------------------------------------------
// BrainSwitcher
// ---------------------------------------------------------------------------

/// Sidebar widget that discovers all repositories accessible to the current
/// user and shows their Brain status (`accessible` / `missing-config`).
/// Displayed collapsed; expands on click to reveal the repo list.
#[component]
pub fn BrainSwitcher(
    /// Active target org for highlighting in the list.
    current_org: Option<String>,
    /// Active target repo for highlighting in the list.
    current_repo: Option<String>,
    /// Active target branch for highlighting in the list.
    current_branch: Option<String>,
) -> impl IntoView {
    let open = RwSignal::new(false);
    // Store in reactive-graph-stable wrappers so closures can borrow across
    // multiple reactive runs without consuming the value.
    let current_org = StoredValue::new(current_org);
    let current_repo = StoredValue::new(current_repo);
    let current_branch = StoredValue::new(current_branch);

    let targets = Resource::new(
        move || open.get(),
        |is_open| async move {
            if is_open {
                list_accessible_targets().await.unwrap_or_default()
            } else {
                vec![]
            }
        },
    );

    let current_label = current_org.with_value(|o| {
        current_repo.with_value(|r| match (o, r) {
            (Some(o), Some(r)) => match current_branch.get_value() {
                Some(b) => format!("{}/{}/{}", o, r, b),
                None => format!("{}/{}", o, r),
            },
            _ => "Switch repo".to_string(),
        })
    });

    view! {
        <div class="border-b border-slate-800 pb-4 mb-2">
            <button
                class="w-full flex items-center justify-between px-1 py-1.5 text-xs text-slate-400 hover:text-slate-200 transition-colors focus:outline-none"
                on:click=move |_| open.update(|v| *v = !*v)
            >
                <span class="flex items-center gap-1.5">
                    <svg class="w-3 h-3 text-teal-400 shrink-0" viewBox="0 0 16 16" fill="currentColor">
                        <path d="M2 2.5A2.5 2.5 0 014.5 0h7A2.5 2.5 0 0114 2.5v10.795a.5.5 0 01-.724.447L8 11.24l-5.276 2.502A.5.5 0 012 13.295V2.5z"/>
                    </svg>
                    <span class="font-medium truncate max-w-[160px]">{current_label}</span>
                </span>
                <svg
                    class="w-3 h-3 shrink-0 transition-transform"
                    class=("rotate-180", move || open.get())
                    viewBox="0 0 16 16" fill="currentColor"
                >
                    <path d="M1.646 4.646a.5.5 0 01.708 0L8 10.293l5.646-5.647a.5.5 0 01.708.708l-6 6a.5.5 0 01-.708 0l-6-6a.5.5 0 010-.708z"/>
                </svg>
            </button>

            <Show when=move || open.get()>
                <Suspense fallback=|| view! {
                    <p class="text-[10px] text-slate-500 px-1 py-2">"Discovering repos…"</p>
                }>
                    {move || {
                        let list = targets.get().unwrap_or_default();
                        view! {
                            <div class="max-h-64 overflow-y-auto">
                            {if list.is_empty() {
                                view! {
                                    <p class="text-[10px] text-slate-500 px-1 py-2">"No accessible repos found."</p>
                                }.into_any()
                            } else {
                                list.into_iter().map(|t| {
                                    let is_current = current_org.with_value(|o| o.as_deref() == Some(&t.org))
                                        && current_repo.with_value(|r| r.as_deref() == Some(&t.repo))
                                        && current_branch.with_value(|b| b.as_deref().is_none_or(|branch| branch == t.active_branch));
                                    let label = format!("{}/{} · {}", t.org, t.repo, t.active_branch);
                                    let state_label = match &t.state {
                                        AccessibleTargetState::Accessible => "accessible",
                                        AccessibleTargetState::MissingConfig => "missing config",
                                        AccessibleTargetState::Forbidden => "forbidden",
                                        AccessibleTargetState::BranchMissing => "branch missing",
                                        AccessibleTargetState::ConfigInvalid => "config invalid",
                                    };
                                    let branch_differs = t.active_branch != t.default_branch;
                                    if matches!(t.state, AccessibleTargetState::Accessible) {
                                        let href = format!(
                                            "/{}/{}/{}/knowledge",
                                            t.org,
                                            t.repo,
                                            encode_path_segment(&t.active_branch),
                                        );
                                        view! {
                                            <a
                                                href=href
                                                rel="external"
                                                class="flex items-center gap-2 px-1 py-1 rounded text-[11px] transition-colors"
                                                class=("text-teal-200", is_current)
                                                class=("bg-teal-500/10", is_current)
                                                class=("text-slate-400", !is_current)
                                                class=("hover:text-slate-200", !is_current)
                                            >
                                                <span
                                                    class="inline-block w-1.5 h-1.5 rounded-full shrink-0 bg-teal-400"
                                                    title="Brain config present"
                                                ></span>
                                                <span class="truncate">{label}</span>
                                                {branch_differs.then(|| view! {
                                                    <span
                                                        class="ml-auto rounded border border-amber-400/40 px-1 py-0.5 text-[9px] uppercase tracking-widest text-amber-200"
                                                        title=format!("Repo default branch is {}", t.default_branch)
                                                    >
                                                        "branch"
                                                    </span>
                                                })}
                                            </a>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <div
                                                class="flex items-center gap-2 px-1 py-1 rounded text-[11px] text-slate-600 cursor-not-allowed"
                                                title=format!("Brain target state: {state_label}")
                                            >
                                                <span
                                                    class="inline-block w-1.5 h-1.5 rounded-full shrink-0 bg-slate-700"
                                                ></span>
                                                <span class="truncate">{label}</span>
                                                <span class="ml-auto text-[9px] uppercase tracking-widest text-slate-600">
                                                    {state_label}
                                                </span>
                                            </div>
                                        }.into_any()
                                    }
                                }).collect_view().into_any()
                            }}
                            </div>
                        }
                    }}
                </Suspense>
            </Show>
        </div>
    }
}
