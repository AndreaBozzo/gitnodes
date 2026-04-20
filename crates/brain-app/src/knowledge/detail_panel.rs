use leptos::prelude::*;

use super::components::TagBadge;
use super::types::{Edge, EditMode, EditPrefill, Node, NodeType};
use crate::api::{AppConfig, BrainFile, read_brain_file};
#[cfg(not(feature = "ssr"))]
use crate::api::{delete_brain_file, rename_brain_file};

#[component]
pub fn DetailPanel(
    nodes: StoredValue<Vec<Node>>,
    edges: StoredValue<Vec<Edge>>,
    selected: RwSignal<Option<u32>>,
    edit_mode: RwSignal<EditMode>,
    graph_version: RwSignal<u64>,
) -> impl IntoView {
    let current = move || {
        selected
            .get()
            .and_then(|id| nodes.with_value(|ns| ns.iter().find(|n| n.id == id).cloned()))
    };

    let app_config = use_context::<Resource<Result<AppConfig, ServerFnError>>>();
    let blob_base = Memo::new(move |_| {
        app_config
            .and_then(|r| r.get())
            .and_then(|r| r.ok())
            .map(|c| c.target.blob_base())
            .unwrap_or_default()
    });

    let current_path = Memo::new(move |_| current().map(|n| n.path).unwrap_or_default());

    let file = Resource::new(
        move || current_path.get(),
        |path| async move {
            if path.is_empty() {
                return Ok::<Option<BrainFile>, ServerFnError>(None);
            }
            read_brain_file(path).await.map(Some)
        },
    );

    let deleting = RwSignal::new(false);
    let delete_error = RwSignal::new(String::new());
    let rename_input = RwSignal::new(Option::<String>::None);
    let rename_msg = RwSignal::new(String::new());
    let renaming = RwSignal::new(false);
    let rename_error = RwSignal::new(String::new());
    let rename_status = RwSignal::new(String::new());
    let delete_msg = RwSignal::new(String::new());
    // `Some(_)` while the delete-confirm banner is open; carries the list of
    // `(title, path)` pairs of docs that link TO the current node. Empty vec
    // means "no backlinks, but still confirm before committing a delete".
    let delete_prompt = RwSignal::new(Option::<Vec<(String, String)>>::None);

    // Backlinks for the currently-selected doc: docs that link TO it, excluding
    // virtual tag nodes (they'd produce noisy "don't worry about the tag"
    // warnings — tag nodes disappear automatically once docs stop referencing
    // them). Edges are undirected, so we match on either endpoint.
    let backlinks = Memo::new(move |_| {
        let Some(id) = selected.get() else {
            return Vec::<(String, String)>::new();
        };
        nodes.with_value(|ns| {
            edges.with_value(|es| {
                es.iter()
                    .filter_map(|e| {
                        let other = if e.from == id {
                            e.to
                        } else if e.to == id {
                            e.from
                        } else {
                            return None;
                        };
                        ns.iter().find(|n| n.id == other)
                    })
                    .filter(|n| n.node_type != NodeType::Tag && !n.path.is_empty())
                    .map(|n| (n.title.clone(), n.path.clone()))
                    .collect()
            })
        })
    });

    let loaded_file = move || match file.get() {
        Some(Ok(Some(bf))) => Some(bf),
        _ => None,
    };
    let loaded_sha = move || loaded_file().map(|bf| bf.sha);

    let request_delete = move || {
        delete_error.set(String::new());
        delete_prompt.set(Some(backlinks.get_untracked()));
    };
    let cancel_delete = move || {
        delete_prompt.set(None);
    };
    let confirm_delete = move |path: String, sha: String| {
        delete_prompt.set(None);
        #[cfg(not(feature = "ssr"))]
        {
            deleting.set(true);
            delete_error.set(String::new());
            let path_for_task = path.clone();
            let msg = {
                let m = delete_msg.get_untracked();
                let t = m.trim();
                if t.is_empty() {
                    None
                } else {
                    Some(t.to_string())
                }
            };
            delete_msg.set(String::new());
            leptos::task::spawn_local(async move {
                match delete_brain_file(path_for_task, sha, msg).await {
                    Ok(()) => {
                        graph_version.update(|v| *v += 1);
                    }
                    Err(e) => {
                        delete_error.set(format!("Delete failed: {e}"));
                        deleting.set(false);
                    }
                }
            });
        }
        #[cfg(feature = "ssr")]
        {
            let _ = (path, sha, &graph_version);
        }
    };

    view! {
        <Show when=move || current().map(|n| !n.path.is_empty()).unwrap_or(false)>
            {move || {
                let node = current().expect("guarded by Show");
                let accent = node.node_type.accent_var().to_string();
                let label = node.node_type.label();
                let title = node.title.clone();
                let tags = node.tags.clone();
                let path = node.path.clone();
                let path_for_rename_block = path.clone();
                let github_url = {
                    let base = blob_base.get();
                    if base.is_empty() {
                        String::new()
                    } else {
                        format!("{}/{}", base, path)
                    }
                };
                view! {
                    <aside class="w-[520px] shrink-0 border-l border-slate-800 bg-slate-950 flex flex-col min-h-0">
                        <div class="p-6 border-b border-slate-800 flex items-start gap-3">
                            <div
                                class="w-2 h-2 rounded-full mt-2 shrink-0"
                                style=format!("background:{}", accent)
                            ></div>
                            <div class="flex-1 min-w-0">
                                <div class="text-[10px] uppercase tracking-widest text-slate-500">
                                    {label}
                                </div>
                                <h2 class="text-base font-semibold text-slate-100 mt-0.5 break-words">
                                    {title}
                                </h2>
                                <div class="flex flex-wrap gap-1 mt-2">
                                    {tags.iter().map(|t| {
                                        view! { <TagBadge tag=t.clone() /> }
                                    }).collect_view()}
                                </div>
                                <div class="flex items-center gap-3 mt-3 text-[11px]">
                                    <a
                                        href=github_url
                                        target="_blank"
                                        rel="noreferrer"
                                        class="text-teal-300 hover:text-teal-200"
                                    >
                                        "View on GitHub ↗"
                                    </a>
                                    <span class="text-slate-600">"·"</span>
                                    <span class="text-slate-500 truncate">{path.clone()}</span>
                                </div>
                            </div>
                            <div class="flex items-center gap-2 shrink-0">
                                {
                                    let path_for_edit = path.clone();
                                    view! {
                                        <button
                                            class="px-2 py-1 rounded text-[10px] uppercase tracking-widest border border-teal-400/40 text-teal-200 hover:bg-teal-500/10 transition-colors focus:outline-none focus:ring-1 focus:ring-teal-500 disabled:opacity-40 disabled:cursor-not-allowed"
                                            aria-label="Edit"
                                            disabled=move || loaded_file().is_none()
                                            on:click=move |_| {
                                                if let Some(bf) = loaded_file() {
                                                    let prefill = EditPrefill::from_raw(
                                                        &path_for_edit,
                                                        &bf.sha,
                                                        &bf.content,
                                                    );
                                                    edit_mode.set(EditMode::Edit(prefill));
                                                }
                                            }
                                        >
                                            "Edit"
                                        </button>
                                    }
                                }
                                {
                                    let path_for_rename = path.clone();
                                    view! {
                                        <button
                                            class="px-2 py-1 rounded text-[10px] uppercase tracking-widest border border-slate-600 text-slate-300 hover:bg-slate-800 transition-colors focus:outline-none focus:ring-1 focus:ring-slate-500 disabled:opacity-40 disabled:cursor-not-allowed"
                                            aria-label="Rename"
                                            disabled=move || renaming.get() || loaded_sha().is_none() || rename_input.with(|r| r.is_some())
                                            on:click=move |_| {
                                                rename_error.set(String::new());
                                                rename_status.set(String::new());
                                                rename_input.set(Some(path_for_rename.clone()));
                                            }
                                        >
                                            {move || if renaming.get() { "Renaming…" } else { "Rename" }}
                                        </button>
                                    }
                                }
                                {
                                    view! {
                                        <button
                                            class="px-2 py-1 rounded text-[10px] uppercase tracking-widest border border-rose-500/40 text-rose-300 hover:bg-rose-500/10 transition-colors focus:outline-none focus:ring-1 focus:ring-rose-500 disabled:opacity-40 disabled:cursor-not-allowed"
                                            aria-label="Delete"
                                            disabled=move || deleting.get() || loaded_sha().is_none() || delete_prompt.with(|p| p.is_some())
                                            on:click=move |_| request_delete()
                                        >
                                            {move || if deleting.get() { "Deleting…" } else { "Delete" }}
                                        </button>
                                    }
                                }
                                <button
                                    class="text-slate-500 hover:text-slate-200 text-lg leading-none transition-colors focus:outline-none focus:ring-1 focus:ring-slate-500 rounded px-1"
                                    aria-label="Close"
                                    on:click=move |_| selected.set(None)
                                >
                                    "×"
                                </button>
                            </div>
                        </div>
                        <Show when=move || rename_input.with(|r| r.is_some())>
                            {
                                let old_path = path_for_rename_block.clone();
                                view! {
                                    <div class="px-6 py-3 border-b border-slate-800 bg-slate-900/60 text-[12px] space-y-2">
                                        <label class="text-[10px] uppercase tracking-widest text-slate-500 block">
                                            "New path (repo-relative, must end in .md)"
                                        </label>
                                        <input
                                            type="text"
                                            class="w-full px-3 py-2 rounded-md bg-slate-800 border border-slate-700 text-slate-100 text-sm focus:border-teal-400 focus:outline-none font-mono"
                                            prop:value=move || rename_input.get().unwrap_or_default()
                                            on:input=move |ev| {
                                                rename_input.set(Some(event_target_value(&ev)));
                                            }
                                        />
                                        <label class="text-[10px] uppercase tracking-widest text-slate-500 block pt-1">
                                            "Commit message (optional)"
                                        </label>
                                        <input
                                            type="text"
                                            maxlength="200"
                                            class="w-full px-3 py-2 rounded-md bg-slate-800 border border-slate-700 text-slate-100 text-sm focus:border-teal-400 focus:outline-none"
                                            placeholder="Leave blank for auto-generated message"
                                            prop:value=move || rename_msg.get()
                                            on:input=move |ev| rename_msg.set(event_target_value(&ev))
                                        />
                                        <Show when=move || !rename_error.get().is_empty()>
                                            <div class="text-rose-300">{move || rename_error.get()}</div>
                                        </Show>
                                        <div class="flex gap-2">
                                            {
                                                let old_for_btn = old_path.clone();
                                                view! {
                                                    <button
                                                        class="px-3 py-1 rounded bg-teal-500/30 border border-teal-400/50 text-teal-50 hover:bg-teal-500/50 transition-colors focus:outline-none focus:ring-1 focus:ring-teal-500 disabled:opacity-40"
                                                        disabled=move || renaming.get() || loaded_sha().is_none()
                                                        on:click=move |_| {
                                                            let Some(target) = rename_input.get_untracked() else {
                                                                return;
                                                            };
                                                            let Some(sha) = loaded_sha() else {
                                                                return;
                                                            };
                                                            if target.trim() == old_for_btn || target.trim().is_empty() {
                                                                rename_error.set(
                                                                    "Pick a different path.".to_string(),
                                                                );
                                                                return;
                                                            }
                                                            #[cfg(not(feature = "ssr"))]
                                                            {
                                                                renaming.set(true);
                                                                rename_error.set(String::new());
                                                                let old_p = old_for_btn.clone();
                                                                let msg = {
                                                                    let m = rename_msg.get_untracked();
                                                                    let t = m.trim();
                                                                    if t.is_empty() { None } else { Some(t.to_string()) }
                                                                };
                                                                leptos::task::spawn_local(async move {
                                                                    match rename_brain_file(old_p, target.clone(), sha, msg).await {
                                                                        Ok(r) => {
                                                                            rename_status.set(format!(
                                                                                "Renamed to {} · rewrote {} referrer{}.",
                                                                                r.new_path,
                                                                                r.updated_referrers.len(),
                                                                                if r.updated_referrers.len() == 1 { "" } else { "s" },
                                                                            ));
                                                                            rename_input.set(None);
                                                                            rename_msg.set(String::new());
                                                                            renaming.set(false);
                                                                            graph_version.update(|v| *v += 1);
                                                                        }
                                                                        Err(e) => {
                                                                            rename_error.set(format!("Rename failed: {e}"));
                                                                            renaming.set(false);
                                                                        }
                                                                    }
                                                                });
                                                            }
                                                            #[cfg(feature = "ssr")]
                                                            {
                                                                let _ = (target, sha, &graph_version);
                                                            }
                                                        }
                                                    >
                                                        {move || if renaming.get() { "Renaming…" } else { "Rename & rewrite links" }}
                                                    </button>
                                                }
                                            }
                                            <button
                                                class="px-3 py-1 rounded bg-slate-800 border border-slate-700 text-slate-300 hover:text-slate-100 transition-colors focus:outline-none focus:ring-1 focus:ring-slate-500"
                                                disabled=move || renaming.get()
                                                on:click=move |_| {
                                                    rename_input.set(None);
                                                    rename_error.set(String::new());
                                                }
                                            >
                                                "Cancel"
                                            </button>
                                        </div>
                                        <p class="text-[10px] text-slate-500">
                                            "Each referring file gets its own commit — expect N+2 commits."
                                        </p>
                                    </div>
                                }
                            }
                        </Show>
                        <Show when=move || !rename_status.get().is_empty()>
                            <div class="px-6 py-2 text-[11px] text-teal-300 border-b border-teal-500/20 bg-teal-500/5">
                                {move || rename_status.get()}
                            </div>
                        </Show>
                        <Show when=move || !delete_error.get().is_empty()>
                            <div class="px-6 py-2 text-[11px] text-rose-300 border-b border-rose-500/20 bg-rose-500/5">
                                {move || delete_error.get()}
                            </div>
                        </Show>
                        <Show when=move || delete_prompt.with(|p| p.is_some())>
                            {
                                let path_for_confirm = path.clone();
                                view! {
                                    <div class="px-6 py-3 border-b border-rose-500/30 bg-rose-500/10 text-[12px] text-rose-100 space-y-2">
                                        <div class="font-semibold">
                                            {move || {
                                                let n = delete_prompt
                                                    .with(|p| p.as_ref().map(|v| v.len()).unwrap_or(0));
                                                if n == 0 {
                                                    "Delete this file? This commits to the Brain repo.".to_string()
                                                } else {
                                                    format!(
                                                        "⚠ {n} other doc{} link{} to this file. Deleting will leave broken links:",
                                                        if n == 1 { "" } else { "s" },
                                                        if n == 1 { "s" } else { "" },
                                                    )
                                                }
                                            }}
                                        </div>
                                        <Show when=move || delete_prompt
                                            .with(|p| p.as_ref().map(|v| !v.is_empty()).unwrap_or(false))>
                                            <ul class="list-disc list-inside space-y-0.5 text-rose-200 max-h-32 overflow-y-auto">
                                                {move || delete_prompt
                                                    .get()
                                                    .unwrap_or_default()
                                                    .into_iter()
                                                    .map(|(title, path)| view! {
                                                        <li>
                                                            <span class="font-medium">{title}</span>
                                                            <span class="text-rose-300/70 ml-1">"("{path}")"</span>
                                                        </li>
                                                    })
                                                    .collect_view()}
                                            </ul>
                                        </Show>
                                        <input
                                            type="text"
                                            maxlength="200"
                                            class="w-full px-3 py-2 rounded-md bg-slate-900 border border-rose-500/30 text-slate-100 text-sm focus:border-rose-400 focus:outline-none"
                                            placeholder="Commit message (optional) — leave blank for auto"
                                            prop:value=move || delete_msg.get()
                                            on:input=move |ev| delete_msg.set(event_target_value(&ev))
                                        />
                                        <div class="flex gap-2 pt-1">
                                            {
                                                let path_for_btn = path_for_confirm.clone();
                                                view! {
                                                    <button
                                                        class="px-3 py-1 rounded bg-rose-500/30 border border-rose-400/50 text-rose-50 hover:bg-rose-500/50 transition-colors focus:outline-none focus:ring-1 focus:ring-rose-500 disabled:opacity-40"
                                                        disabled=move || loaded_sha().is_none()
                                                        on:click=move |_| {
                                                            if let Some(sha) = loaded_sha() {
                                                                confirm_delete(path_for_btn.clone(), sha);
                                                            }
                                                        }
                                                    >
                                                        "Delete anyway"
                                                    </button>
                                                }
                                            }
                                            <button
                                                class="px-3 py-1 rounded bg-slate-800 border border-slate-700 text-slate-300 hover:text-slate-100 transition-colors focus:outline-none focus:ring-1 focus:ring-slate-500"
                                                on:click=move |_| { delete_msg.set(String::new()); cancel_delete(); }
                                            >
                                                "Cancel"
                                            </button>
                                        </div>
                                    </div>
                                }
                            }
                        </Show>
                        <div class="flex-1 overflow-y-auto p-6">
                            <Suspense fallback=move || view! {
                                <div class="text-slate-500 text-xs">"Loading document…"</div>
                            }>
                                {move || match file.get() {
                                    None => ().into_any(),
                                    Some(Err(e)) => view! {
                                        <div class="text-amber-300 text-xs">
                                            {format!("Failed to load: {e}")}
                                        </div>
                                    }.into_any(),
                                    Some(Ok(None)) => view! {
                                        <div class="text-slate-500 text-xs">
                                            "No file backs this node."
                                        </div>
                                    }.into_any(),
                                    Some(Ok(Some(bf))) => view! {
                                        <article
                                            class="prose prose-invert max-w-prose"
                                            inner_html=bf.rendered_html
                                        ></article>
                                    }.into_any(),
                                }}
                            </Suspense>
                        </div>
                    </aside>
                }
            }}
        </Show>
    }
}
