use leptos::prelude::*;

use super::components::TagBadge;
use super::types::{Edge, EditMode, EditPrefill, Node};
use crate::api::{
    BrainFile, WorkItemComment, WriteMode, assign_work_item, bind_work_item,
    load_work_item_by_path, load_work_item_comments, read_brain_file, transition_work_item,
};
#[cfg(not(feature = "ssr"))]
use crate::api::{delete_brain_file, rename_brain_file};
use brain_domain::{
    ExternalWorkItemBinding, ExternalWorkItemSystem, TargetRef, WorkItem, WorkItemState,
    WorkItemSystemOfRecord,
};

/// Backlinks bucketed by node type, ordered by `config.node_types` first, then
/// any unknown node types. Each bucket holds `(title, path)` pairs ready for
/// the panel to render.
type BacklinkGroups = Vec<(brain_domain::NodeTypeSpec, Vec<(String, String)>)>;
type BacklinkEntry = (String, String, String);

#[component]
pub fn DetailPanel(
    nodes: StoredValue<Vec<Node>>,
    edges: StoredValue<Vec<Edge>>,
    selected: RwSignal<Option<u32>>,
    selected_path: RwSignal<Option<String>>,
    active_path_prefix: RwSignal<Option<String>>,
    edit_mode: RwSignal<EditMode>,
    graph_version: RwSignal<u64>,
    config: brain_domain::BrainConfig,
) -> impl IntoView {
    let active_target = StoredValue::new(expect_context::<TargetRef>());
    let config = StoredValue::new(config);
    let current = move || {
        selected
            .get()
            .and_then(|id| nodes.with_value(|ns| ns.iter().find(|n| n.id == id).cloned()))
    };

    let blob_base = Memo::new(move |_| {
        let target: brain_domain::TargetConfig = active_target.get_value().into();
        brain_domain::GithubClient::new(target).blob_base()
    });

    let current_path = Memo::new(move |_| selected_path.get().unwrap_or_default());

    let file = Resource::new(
        move || current_path.get(),
        move |path| async move {
            if path.is_empty() {
                return Ok::<Option<BrainFile>, crate::api::ApiError>(None);
            }
            read_brain_file(active_target.get_value(), path)
                .await
                .map(Some)
        },
    );
    let work_item = Resource::new(
        move || current_path.get(),
        move |path| async move {
            if path.is_empty() {
                return Ok::<Option<WorkItem>, crate::api::ApiError>(None);
            }
            load_work_item_by_path(active_target.get_value(), path).await
        },
    );

    let deleting = RwSignal::new(false);
    let image_modal = RwSignal::new(Option::<(String, String)>::None);
    let mermaid_modal = RwSignal::new(Option::<String>::None);
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
    //
    // Each entry carries `(node_type, title, path)` so the panel can group by
    // type without re-reading the graph. The delete-confirm consumer drops
    // `node_type` via `backlinks_titles` below.
    let backlinks = Memo::new(move |_| {
        let Some(id) = selected.get() else {
            return Vec::<BacklinkEntry>::new();
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
                    .filter(|n| {
                        let is_tag = config.with_value(|c| {
                            c.synthetic_tag_spec().map(|s| s.name.as_str())
                                == Some(n.node_type.as_str())
                        });
                        !is_tag && !n.path.is_empty()
                    })
                    .map(|n| (n.node_type.clone(), n.title.clone(), n.path.clone()))
                    .collect()
            })
        })
    });

    // Backlinks grouped by node type, in `config.node_types` order. Each group
    // holds `(title, path)` pairs. Tag-only siblings are already filtered out
    // upstream, so an empty result here means "no incoming links from any
    // typed doc."
    let grouped_backlinks: Memo<BacklinkGroups> = Memo::new(move |_| {
        let entries = backlinks.get();
        if entries.is_empty() {
            return BacklinkGroups::new();
        }
        config.with_value(|c| group_backlinks(entries, c))
    });

    let loaded_file = move || match file.get() {
        Some(Ok(Some(bf))) => Some(bf),
        _ => None,
    };
    let loaded_sha = move || loaded_file().map(|bf| bf.sha);

    #[cfg(feature = "hydrate")]
    {
        Effect::new(move |_| {
            if file.get().is_some() {
                crate::knowledge::mermaid::render_brain_mermaid();
            }
        });
    }

    let request_delete = move || {
        delete_error.set(String::new());
        // delete-confirm panel only needs (title, path) — drop the node_type
        // we added for grouping.
        let titles_paths: Vec<(String, String)> = backlinks
            .get_untracked()
            .into_iter()
            .map(|(_node_type, title, path)| (title, path))
            .collect();
        delete_prompt.set(Some(titles_paths));
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
                match delete_brain_file(active_target.get_value(), path_for_task, sha, msg).await {
                    Ok(result) => {
                        if result.mode == WriteMode::Direct {
                            selected_path.set(None);
                            graph_version.update(|v| *v += 1);
                        } else {
                            delete_error.set(format!(
                                "Proposed deletion via PR #{}.",
                                result
                                    .pr_number
                                    .map(|n| n.to_string())
                                    .unwrap_or_else(|| "?".to_string())
                            ));
                            deleting.set(false);
                        }
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
    let open_clicked_visual = move |ev: leptos::ev::MouseEvent| {
        #[cfg(feature = "hydrate")]
        {
            use wasm_bindgen::JsCast;

            let Some(target) = ev
                .target()
                .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
            else {
                return;
            };
            if target.tag_name().eq_ignore_ascii_case("img") {
                let Some(src) = target.get_attribute("src").filter(|src| !src.is_empty()) else {
                    return;
                };
                let alt = target.get_attribute("alt").unwrap_or_default();
                image_modal.set(Some((src, alt)));
                return;
            }
            if let Ok(Some(diagram)) = target.closest(".mermaid") {
                mermaid_modal.set(Some(diagram.outer_html()));
            }
        }
        #[cfg(not(feature = "hydrate"))]
        {
            let _ = ev;
        }
    };

    view! {
        <Show when=move || current().map(|n| !n.path.is_empty()).unwrap_or(false)>
            {move || {
                let node = current().expect("guarded by Show");
                let spec = config.with_value(|c| c.lookup(&node.node_type).unwrap_or_else(|| c.default_spec()).clone());
                let accent = spec.accent_var();
                let label = spec.label.clone();
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
                                        href=github_url.clone()
                                        target="_blank"
                                        rel="noreferrer"
                                        class="text-teal-300 hover:text-teal-200"
                                    >
                                        "View on GitHub ↗"
                                    </a>
                                    <span class="text-slate-600">"·"</span>
                                    <PathBreadcrumb
                                        path=path.clone()
                                        github_url=github_url.clone()
                                        active_path_prefix=active_path_prefix
                                    />
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
                                                    let prefill = config.with_value(|c| {
                                                        EditPrefill::from_raw(
                                                            &path_for_edit,
                                                            &bf.sha,
                                                            &bf.content,
                                                            c,
                                                        )
                                                    });
                                                    edit_mode.set(EditMode::Edit(Box::new(prefill)));
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
                                    on:click=move |_| {
                                        selected.set(None);
                                        selected_path.set(None);
                                    }
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
                                                                    match rename_brain_file(active_target.get_value(), old_p, target.clone(), sha, msg).await {
                                                                        Ok(r) => {
                                                                            match r.write.mode {
                                                                                WriteMode::Direct => {
                                                                                    rename_status.set(format!(
                                                                                        "Renamed to {} · rewrote {} referrer{}.",
                                                                                        r.new_path,
                                                                                        r.updated_referrers.len(),
                                                                                        if r.updated_referrers.len() == 1 { "" } else { "s" },
                                                                                    ));
                                                                                    selected_path.set(Some(r.new_path.clone()));
                                                                                    graph_version.update(|v| *v += 1);
                                                                                }
                                                                                WriteMode::PullRequest => {
                                                                                    rename_status.set(format!(
                                                                                        "Proposed rename via PR #{} · would rewrite {} referrer{}.",
                                                                                        r.write
                                                                                            .pr_number
                                                                                            .map(|n| n.to_string())
                                                                                            .unwrap_or_else(|| "?".to_string()),
                                                                                        r.updated_referrers.len(),
                                                                                        if r.updated_referrers.len() == 1 { "" } else { "s" },
                                                                                    ));
                                                                                }
                                                                            }
                                                                            rename_input.set(None);
                                                                            rename_msg.set(String::new());
                                                                            renaming.set(false);
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
                                            "Rename uses one atomic Git Data API commit and rewrites links before the projection refetch."
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
                                    None => view! {
                                        <div class="text-slate-500 text-xs">"Loading document…"</div>
                                    }.into_any(),
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
                                    Some(Ok(Some(bf))) => {
                                        let cover_url = bf.cover_url.clone();
                                        let cover_alt = bf
                                            .cover_alt
                                            .clone()
                                            .unwrap_or_else(|| node.title.clone());
                                        view! {
                                        <>
                                            <Show when=move || matches!(work_item.get(), Some(Ok(Some(_))))>
                                                {move || match work_item.get() {
                                                    Some(Ok(Some(item))) => view! {
                                                        <WorkItemCard item=item graph_version=graph_version />
                                                    }.into_any(),
                                                    _ => ().into_any(),
                                                }}
                                            </Show>
                                            {cover_url.clone().map(|src| {
                                                let src_attr = src.clone();
                                                let src_for_modal = src;
                                                let alt_attr = cover_alt.clone();
                                                let modal_alt = cover_alt.clone();
                                                view! {
                                                    <button
                                                        type="button"
                                                        class="block w-full mb-4 rounded-md overflow-hidden border border-slate-800 bg-slate-900 focus:outline-none focus:ring-1 focus:ring-teal-500"
                                                        aria-label="Open cover image"
                                                        on:click=move |_| image_modal.set(
                                                            Some((src_for_modal.clone(), modal_alt.clone()))
                                                        )
                                                    >
                                                        <img
                                                            src=src_attr
                                                            alt=alt_attr
                                                            class="w-full max-h-[260px] object-cover"
                                                            loading="lazy"
                                                        />
                                                    </button>
                                                }
                                            })}
                                            <article
                                                class="prose prose-invert max-w-prose"
                                                on:click=open_clicked_visual
                                                inner_html=bf.rendered_html
                                            ></article>
                                            <BacklinksSection
                                                grouped=grouped_backlinks
                                                nodes=nodes
                                                selected=selected
                                                selected_path=selected_path
                                            />
                                            <Show when=move || image_modal.with(|image| image.is_some())>
                                                {move || {
                                                    let (src, alt) = image_modal.get().unwrap_or_default();
                                                    let caption = alt.clone();
                                                    let has_caption = !caption.is_empty();
                                                    view! {
                                                        <div
                                                            class="fixed inset-0 z-50 bg-slate-950/90 p-6 flex items-center justify-center"
                                                            role="dialog"
                                                            aria-modal="true"
                                                            on:click=move |_| image_modal.set(None)
                                                        >
                                                            <div
                                                                class="max-w-[92vw] max-h-[92vh] flex flex-col gap-3"
                                                                on:click=move |ev| ev.stop_propagation()
                                                            >
                                                                <div class="flex justify-end">
                                                                    <button
                                                                        type="button"
                                                                        class="px-2 py-1 rounded text-[10px] uppercase tracking-widest border border-slate-600 text-slate-200 hover:bg-slate-800 transition-colors focus:outline-none focus:ring-1 focus:ring-slate-500"
                                                                        aria-label="Close image preview"
                                                                        on:click=move |_| image_modal.set(None)
                                                                    >
                                                                        "Close"
                                                                    </button>
                                                                </div>
                                                                <img
                                                                    src=src
                                                                    alt=alt
                                                                    class="max-w-[92vw] max-h-[84vh] object-contain rounded border border-slate-700 bg-slate-900"
                                                                />
                                                                <Show when=move || has_caption>
                                                                    <div class="text-xs text-slate-300 text-center">
                                                                        {caption.clone()}
                                                                    </div>
                                                                </Show>
                                                            </div>
                                                        </div>
                                                    }
                                                }}
                                            </Show>
                                            <Show when=move || mermaid_modal.with(|diagram| diagram.is_some())>
                                                {move || {
                                                    let diagram = mermaid_modal.get().unwrap_or_default();
                                                    view! {
                                                        <div
                                                            class="fixed inset-0 z-50 bg-slate-950/90 p-4 sm:p-6 flex items-center justify-center"
                                                            role="dialog"
                                                            aria-modal="true"
                                                            on:click=move |_| mermaid_modal.set(None)
                                                        >
                                                            <div
                                                                class="w-[min(96vw,1400px)] h-[min(90vh,960px)] flex flex-col gap-3"
                                                                on:click=move |ev| ev.stop_propagation()
                                                            >
                                                                <div class="flex justify-end">
                                                                    <button
                                                                        type="button"
                                                                        class="px-2 py-1 rounded text-[10px] uppercase tracking-widest border border-slate-600 text-slate-200 hover:bg-slate-800 transition-colors focus:outline-none focus:ring-1 focus:ring-slate-500"
                                                                        aria-label="Close diagram preview"
                                                                        on:click=move |_| mermaid_modal.set(None)
                                                                    >
                                                                        "Close"
                                                                    </button>
                                                                </div>
                                                                <div
                                                                    class="flex-1 min-h-0 overflow-auto rounded border border-slate-700 bg-slate-950 p-4 sm:p-6"
                                                                >
                                                                    <div
                                                                        class="mermaid-modal-diagram"
                                                                        inner_html=diagram
                                                                    ></div>
                                                                </div>
                                                            </div>
                                                        </div>
                                                    }
                                                }}
                                            </Show>
                                        </>
                                    }.into_any()
                                    },
                                }}
                            </Suspense>
                        </div>
                    </aside>
                }
            }}
        </Show>
    }
}

fn group_backlinks(
    entries: Vec<BacklinkEntry>,
    config: &brain_domain::BrainConfig,
) -> BacklinkGroups {
    let mut by_type: std::collections::BTreeMap<String, Vec<(String, String)>> =
        std::collections::BTreeMap::new();
    for (node_type, title, path) in entries {
        by_type.entry(node_type).or_default().push((title, path));
    }
    // Stable within a group: ordered by title for readability.
    for group in by_type.values_mut() {
        group.sort_by_key(|(title, _)| title.to_lowercase());
    }

    let mut groups: BacklinkGroups = config
        .node_types
        .iter()
        .filter_map(|spec| {
            by_type
                .remove(&spec.name)
                .map(|group| (spec.clone(), group))
        })
        .collect();

    let fallback = config.default_spec();
    groups.extend(
        by_type
            .into_iter()
            .map(|(node_type, group)| (unknown_backlink_spec(fallback, &node_type), group)),
    );
    groups
}

fn unknown_backlink_spec(
    fallback: &brain_domain::NodeTypeSpec,
    node_type: &str,
) -> brain_domain::NodeTypeSpec {
    let trimmed = node_type.trim();
    let mut spec = fallback.clone();
    spec.name = if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed.to_string()
    };
    spec.label = if trimmed.is_empty() {
        "Unknown".to_string()
    } else {
        format!("Unknown ({trimmed})")
    };
    spec
}

#[component]
fn BacklinksSection(
    grouped: Memo<BacklinkGroups>,
    nodes: StoredValue<Vec<Node>>,
    selected: RwSignal<Option<u32>>,
    selected_path: RwSignal<Option<String>>,
) -> impl IntoView {
    view! {
        <Show when=move || !grouped.get().is_empty()>
            <section class="mt-6 border-t border-slate-800 pt-4">
                <header class="flex flex-wrap items-baseline gap-2 mb-3">
                    <span class="text-[10px] uppercase tracking-widest text-slate-500">
                        "Linked from"
                    </span>
                    <span class="text-[10px] text-slate-400">
                        {move || {
                            let summary = grouped
                                .get()
                                .iter()
                                .map(|(spec, items)| format!("{} {}", items.len(), spec.label.as_str()))
                                .collect::<Vec<_>>()
                                .join(" · ");
                            summary
                        }}
                    </span>
                </header>
                <div class="flex flex-col gap-3">
                    {move || {
                        grouped
                            .get()
                            .into_iter()
                            .map(|(spec, items)| {
                                let accent = spec.accent_var();
                                let label = spec.label.clone();
                                view! {
                                    <div>
                                        <div class="flex items-center gap-2 mb-1.5">
                                            <span
                                                class="inline-block w-1.5 h-1.5 rounded-full"
                                                style=format!("background:{}", accent)
                                            ></span>
                                            <span class="text-[10px] uppercase tracking-widest text-slate-400">
                                                {label}
                                            </span>
                                            <span class="text-[10px] tabular-nums text-slate-500">
                                                {items.len()}
                                            </span>
                                        </div>
                                        <ul class="space-y-1 ml-3">
                                            {items
                                                .into_iter()
                                                .map(|(title, path)| {
                                                    let path_for_click = path.clone();
                                                    let path_for_lookup = path.clone();
                                                    view! {
                                                        <li>
                                                            <button
                                                                type="button"
                                                                class="text-left text-xs text-slate-300 hover:text-teal-200 focus:outline-none focus:ring-1 focus:ring-slate-500 rounded px-1 -ml-1"
                                                                title=path.clone()
                                                                on:click=move |_| {
                                                                    let id = nodes.with_value(|ns| {
                                                                        ns.iter()
                                                                            .find(|n| n.path == path_for_lookup)
                                                                            .map(|n| n.id)
                                                                    });
                                                                    if let Some(id) = id {
                                                                        selected.set(Some(id));
                                                                    }
                                                                    selected_path.set(Some(path_for_click.clone()));
                                                                }
                                                            >
                                                                {title}
                                                            </button>
                                                        </li>
                                                    }
                                                })
                                                .collect_view()}
                                        </ul>
                                    </div>
                                }
                            })
                            .collect_view()
                    }}
                </div>
            </section>
        </Show>
    }
}

#[component]
fn PathBreadcrumb(
    path: String,
    github_url: String,
    active_path_prefix: RwSignal<Option<String>>,
) -> impl IntoView {
    let parts: Vec<String> = path.split('/').map(ToOwned::to_owned).collect();
    let last_index = parts.len().saturating_sub(1);

    view! {
        <nav class="min-w-0 flex flex-wrap items-center gap-1 text-slate-500" aria-label="File path">
            {parts.into_iter().enumerate().map(move |(idx, part)| {
                let is_file = idx == last_index;
                let prefix = if is_file {
                    String::new()
                } else {
                    let prefix = path
                        .split('/')
                        .take(idx + 1)
                        .collect::<Vec<_>>()
                        .join("/");
                    format!("{prefix}/")
                };
                view! {
                    <>
                        {(idx > 0).then(|| view! { <span class="text-slate-700">"/"</span> })}
                        {if is_file {
                            view! {
                                <a
                                    href=github_url.clone()
                                    target="_blank"
                                    rel="noreferrer"
                                    class="max-w-[220px] truncate font-mono text-slate-400 hover:text-teal-200"
                                    title=path.clone()
                                >
                                    {part}
                                </a>
                            }.into_any()
                        } else {
                            view! {
                                <button
                                    class="font-mono text-slate-500 hover:text-teal-200 focus:outline-none focus:ring-1 focus:ring-slate-600 rounded"
                                    title=format!("Filter to {prefix}")
                                    on:click={
                                        let prefix = prefix.clone();
                                        move |_| active_path_prefix.set(Some(prefix.clone()))
                                    }
                                >
                                    {part}
                                </button>
                            }.into_any()
                        }}
                    </>
                }
            }).collect_view()}
        </nav>
    }
}

#[component]
fn WorkItemCard(item: WorkItem, graph_version: RwSignal<u64>) -> impl IntoView {
    let active_target = expect_context::<TargetRef>();
    let state = work_item_state_label(&item.state);
    let state_class = work_item_state_class(&item.state);
    let system = work_item_system_label(&item.system_of_record);
    let source_class = work_item_source_class(&item.system_of_record);
    let binding = item.external_binding.clone();
    let title = item.title.clone();
    let assignees_view = if item.assignees.is_empty() {
        ().into_any()
    } else {
        let assignees = item.assignees.clone();
        view! {
            <>
                <dt class="text-slate-500 uppercase tracking-widest">"Assignees"</dt>
                <dd class="flex flex-wrap gap-1.5">
                    {assignees.iter().map(|assignee| view! {
                        <span class="rounded-full border border-slate-700 bg-slate-800 px-2 py-0.5 text-[11px] text-slate-200">
                            {assignee.clone()}
                        </span>
                    }).collect_view()}
                </dd>
            </>
        }
        .into_any()
    };
    let labels_view = if item.labels.is_empty() {
        ().into_any()
    } else {
        let labels = item.labels.clone();
        view! {
            <>
                <dt class="text-slate-500 uppercase tracking-widest">"Labels"</dt>
                <dd class="flex flex-wrap gap-1.5">
                    {labels.iter().map(|label| view! {
                        <span class="rounded-full border border-slate-700 bg-slate-800 px-2 py-0.5 text-[11px] text-slate-300">
                            {label.clone()}
                        </span>
                    }).collect_view()}
                </dd>
            </>
        }
        .into_any()
    };
    let binding_view = if let Some(binding) = binding.as_ref() {
        let system = external_system_label(&binding.system).to_string();
        let label = format!("{} · {}#{}", system, binding.project, binding.item_key);
        let url = binding.url.clone();
        view! {
                <>
                    <dt class="text-slate-500 uppercase tracking-widest">"Provider"</dt>
                    <dd>
                        {match url {
                            Some(url) => view! {
                                <a
                                    href=url
                                    target="_blank"
                                    rel="noreferrer"
                                    class="text-teal-300 hover:text-teal-200 underline underline-offset-2"
                                >
                                    {label.clone()}
                                </a>
                            }.into_any(),
                            None => view! {
                                <span class="text-slate-200">{label.clone()}</span>
                            }.into_any(),
                        }}
                    </dd>
                </>
            }
            .into_any()
    } else {
        view! {
            <>
                <dt class="text-slate-500 uppercase tracking-widest">"Provider"</dt>
                <dd class="text-slate-400">"Not bound"</dd>
            </>
        }
        .into_any()
    };
    let sync_view = if binding.is_some() {
        view! {
            <>
                <dt class="text-slate-500 uppercase tracking-widest">"Sync"</dt>
                <dd class="text-slate-300">"Brain changes propagate to the provider; failed pushes surface in Status."</dd>
            </>
        }
        .into_any()
    } else {
        view! {
            <>
                <dt class="text-slate-500 uppercase tracking-widest">"Sync"</dt>
                <dd class="text-slate-400">"Brain-only"</dd>
            </>
        }
        .into_any()
    };

    let brain_id = item.brain_id.clone();
    view! {
        <section class="mb-5 rounded-md border border-slate-800 bg-slate-900/70 p-4 text-sm">
            <div class="flex flex-wrap items-start justify-between gap-3">
                <div class="min-w-0">
                    <div class="flex flex-wrap items-center gap-2">
                        <span class="rounded-full border border-fuchsia-400/30 bg-fuchsia-500/10 px-2.5 py-1 text-[10px] font-semibold uppercase tracking-widest text-fuchsia-200">
                            "Work Item"
                        </span>
                        <span class=state_class>{state}</span>
                    </div>
                    <h3 class="mt-2 text-sm font-semibold text-slate-100 break-words">{title}</h3>
                </div>
                <span class=source_class>{system}</span>
            </div>
            <dl class="mt-3 grid grid-cols-[auto_1fr] gap-x-3 gap-y-2 text-xs text-slate-300">
                <dt class="text-slate-500 uppercase tracking-widest">"Brain ID"</dt>
                <dd class="font-mono text-slate-200 break-all">{item.brain_id.clone()}</dd>
                {assignees_view}
                {labels_view}
                {binding_view}
                {sync_view}
            </dl>
            <WorkItemComments
                target=active_target.clone()
                brain_id=item.brain_id.clone()
                binding=item.external_binding.clone()
                graph_version=graph_version
            />
            <WorkItemControls
                target=active_target.clone()
                brain_id=brain_id
                current_state=item.state.clone()
                current_assignees=item.assignees.clone()
                current_binding=item.external_binding.clone()
                graph_version=graph_version
            />
        </section>
    }
}

#[component]
fn WorkItemComments(
    target: TargetRef,
    brain_id: String,
    binding: Option<ExternalWorkItemBinding>,
    graph_version: RwSignal<u64>,
) -> impl IntoView {
    let is_github_bound = binding
        .as_ref()
        .is_some_and(|b| b.system == ExternalWorkItemSystem::Github);
    if !is_github_bound {
        return ().into_any();
    }

    let comments = Resource::new(
        move || (target.clone(), brain_id.clone(), graph_version.get()),
        |(target, brain_id, _)| async move { load_work_item_comments(target, brain_id).await },
    );

    view! {
        <details class="mt-4 border-t border-slate-800 pt-3">
            <summary class="cursor-pointer text-xs uppercase tracking-widest text-slate-400 hover:text-slate-200">
                "GitHub comments"
            </summary>
            <Suspense fallback=|| view! {
                <p class="mt-3 text-xs text-slate-500">"Loading comments..."</p>
            }>
                {move || match comments.get() {
                    None => view! {
                        <p class="mt-3 text-xs text-slate-500">"Loading comments..."</p>
                    }.into_any(),
                    Some(Err(error)) => view! {
                        <div class="mt-3 rounded-md border border-rose-400/30 bg-rose-500/10 px-3 py-2 text-[11px] text-rose-200">
                            {format!("Failed to load comments: {error}")}
                        </div>
                    }.into_any(),
                    Some(Ok(comments)) if comments.is_empty() => view! {
                        <p class="mt-3 text-xs text-slate-500">"No comments yet."</p>
                    }.into_any(),
                    Some(Ok(comments)) => view! {
                        <ol class="mt-3 space-y-3">
                            {comments.into_iter().map(comment_card).collect_view()}
                        </ol>
                    }.into_any(),
                }}
            </Suspense>
        </details>
    }
    .into_any()
}

fn comment_card(comment: WorkItemComment) -> impl IntoView {
    view! {
        <li class="rounded-md border border-slate-800 bg-slate-950/40 p-3">
            <div class="mb-2 flex flex-wrap items-center gap-2 text-[11px] text-slate-500">
                <a
                    href=comment.author_url
                    target="_blank"
                    rel="noreferrer"
                    class="font-semibold text-slate-300 hover:text-slate-100"
                >
                    {comment.author}
                </a>
                <span>{comment.created_at}</span>
                <a
                    href=comment.url
                    target="_blank"
                    rel="noreferrer"
                    class="text-teal-300 hover:text-teal-200"
                >
                    "Open"
                </a>
            </div>
            <div
                class="prose prose-invert prose-sm max-w-none text-slate-200"
                inner_html=comment.body_html
            ></div>
        </li>
    }
}

/// Inline edit controls for state, assignees, and external binding.
/// Each mutation goes through its own Action so failures stay scoped — a
/// failed binding edit doesn't roll back a successful state transition. On
/// success the global `graph_version` is bumped, which triggers the parent
/// `Resource`s (work_item, file) to refetch.
#[component]
fn WorkItemControls(
    target: TargetRef,
    brain_id: String,
    current_state: WorkItemState,
    current_assignees: Vec<String>,
    current_binding: Option<ExternalWorkItemBinding>,
    graph_version: RwSignal<u64>,
) -> impl IntoView {
    let target_for_state = target.clone();
    let target_for_assign = target.clone();
    let target_for_bind = target.clone();
    let state_action = Action::new(move |args: &(String, WorkItemState)| {
        let (id, state) = args.clone();
        let target = target_for_state.clone();
        async move { transition_work_item(target, id, state).await }
    });
    let assign_action = Action::new(move |args: &(String, Vec<String>)| {
        let (id, list) = args.clone();
        let target = target_for_assign.clone();
        async move { assign_work_item(target, id, list).await }
    });
    let bind_action = Action::new(move |args: &(String, Option<ExternalWorkItemBinding>)| {
        let (id, binding) = args.clone();
        let target = target_for_bind.clone();
        async move { bind_work_item(target, id, binding).await }
    });

    // Bump graph_version only for direct writes. PR fallback writes a branch,
    // not the live target, so the projection should remain anchored to the
    // current default branch until the PR is merged.
    Effect::new(move |_| {
        if matches!(
            state_action.value().get(),
            Some(Ok(result)) if result.write.mode == WriteMode::Direct
        ) {
            graph_version.update(|v| *v += 1);
        }
    });
    Effect::new(move |_| {
        if matches!(
            assign_action.value().get(),
            Some(Ok(result)) if result.write.mode == WriteMode::Direct
        ) {
            graph_version.update(|v| *v += 1);
        }
    });
    Effect::new(move |_| {
        if matches!(
            bind_action.value().get(),
            Some(Ok(result)) if result.write.mode == WriteMode::Direct
        ) {
            graph_version.update(|v| *v += 1);
        }
    });

    let state_signal = RwSignal::new(work_item_state_value(&current_state).to_string());
    let assignees_signal = RwSignal::new(current_assignees.join(", "));
    let bind_open = RwSignal::new(false);
    let initial_binding = current_binding.clone();
    let bind_system = RwSignal::new(
        initial_binding
            .as_ref()
            .map(|b| external_system_value(&b.system).to_string())
            .unwrap_or_else(|| "github".to_string()),
    );
    let bind_project = RwSignal::new(
        initial_binding
            .as_ref()
            .map(|b| b.project.clone())
            .unwrap_or_default(),
    );
    let bind_item_key = RwSignal::new(
        initial_binding
            .as_ref()
            .map(|b| b.item_key.clone())
            .unwrap_or_default(),
    );
    let bind_url = RwSignal::new(
        initial_binding
            .as_ref()
            .and_then(|b| b.url.clone())
            .unwrap_or_default(),
    );

    let state_id = brain_id.clone();
    let assign_id = brain_id.clone();
    let bind_id = brain_id.clone();
    let unbind_id = brain_id.clone();
    let any_pending = move || {
        state_action.pending().get() || assign_action.pending().get() || bind_action.pending().get()
    };

    let any_error = move || {
        let s = state_action.value().get().and_then(|r| r.err());
        let a = assign_action.value().get().and_then(|r| r.err());
        let b = bind_action.value().get().and_then(|r| r.err());
        // Typed boundary error → actionable guidance (stale → reload,
        // no write → PR, rate-limit → retry) instead of a flattened string.
        s.or(a).or(b).map(|e| e.actionable_message())
    };

    let any_notice = move || {
        [
            state_action.value().get(),
            assign_action.value().get(),
            bind_action.value().get(),
        ]
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|result| match result.write.mode {
            WriteMode::Direct => {
                "Saved to Brain. Provider retry status appears under Status if propagation fails."
                    .to_string()
            }
            WriteMode::PullRequest => format!(
                "Proposed via PR #{}. The live Brain updates after that PR is merged.",
                result
                    .write
                    .pr_number
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "?".to_string())
            ),
        })
        .next()
    };

    view! {
        <details class="mt-4 border-t border-slate-800 pt-3">
            <summary class="cursor-pointer text-xs uppercase tracking-widest text-slate-400 hover:text-slate-200">
                "Edit work item"
            </summary>
            <div class="mt-3 flex flex-col gap-4">
                <div class="flex flex-col gap-1.5">
                    <label class="text-[11px] uppercase tracking-widest text-slate-500">"Status"</label>
                    <div class="flex gap-2">
                        <select
                            class="flex-1 rounded-md border border-slate-700 bg-slate-900 px-2 py-1 text-xs text-slate-200"
                            on:change=move |ev| {
                                let v = event_target_value(&ev);
                                state_signal.set(v);
                            }
                            prop:value=move || state_signal.get()
                            disabled=any_pending
                        >
                            <option value="backlog">"Backlog"</option>
                            <option value="todo">"Todo"</option>
                            <option value="in-progress">"In Progress"</option>
                            <option value="blocked">"Blocked"</option>
                            <option value="done">"Done"</option>
                            <option value="cancelled">"Cancelled"</option>
                        </select>
                        <button
                            type="button"
                            class="rounded-md border border-teal-400/40 bg-teal-500/10 px-3 py-1 text-xs text-teal-200 hover:bg-teal-500/20 disabled:opacity-50"
                            disabled=any_pending
                            on:click={
                                let state_id = state_id.clone();
                                move |_| {
                                    let raw = state_signal.get();
                                    let Some(parsed) = parse_state_value(&raw) else { return; };
                                    state_action.dispatch((state_id.clone(), parsed));
                                }
                            }
                        >
                            "Save status"
                        </button>
                    </div>
                </div>

                <div class="flex flex-col gap-1.5">
                    <label class="text-[11px] uppercase tracking-widest text-slate-500">
                        "Assignees (comma-separated)"
                    </label>
                    <div class="flex gap-2">
                        <input
                            type="text"
                            class="flex-1 rounded-md border border-slate-700 bg-slate-900 px-2 py-1 text-xs text-slate-200"
                            prop:value=move || assignees_signal.get()
                            on:input=move |ev| assignees_signal.set(event_target_value(&ev))
                            disabled=any_pending
                        />
                        <button
                            type="button"
                            class="rounded-md border border-teal-400/40 bg-teal-500/10 px-3 py-1 text-xs text-teal-200 hover:bg-teal-500/20 disabled:opacity-50"
                            disabled=any_pending
                            on:click={
                                let assign_id = assign_id.clone();
                                move |_| {
                                    let raw = assignees_signal.get();
                                    let list: Vec<String> = raw
                                        .split(',')
                                        .map(|s| s.trim().to_string())
                                        .filter(|s| !s.is_empty())
                                        .collect();
                                    assign_action.dispatch((assign_id.clone(), list));
                                }
                            }
                        >
                            "Save assignees"
                        </button>
                    </div>
                </div>

                <div class="flex flex-col gap-1.5">
                    <div class="flex items-center justify-between">
                        <label class="text-[11px] uppercase tracking-widest text-slate-500">
                            "External binding"
                        </label>
                        <button
                            type="button"
                            class="text-[11px] text-slate-400 hover:text-slate-200"
                            on:click=move |_| bind_open.update(|o| *o = !*o)
                        >
                            {move || if bind_open.get() { "Hide" } else { "Edit" }}
                        </button>
                    </div>
                    <Show when=move || bind_open.get()>
                        <div class="grid grid-cols-2 gap-2">
                            <select
                                class="rounded-md border border-slate-700 bg-slate-900 px-2 py-1 text-xs text-slate-200"
                                prop:value=move || bind_system.get()
                                on:change=move |ev| bind_system.set(event_target_value(&ev))
                                disabled=any_pending
                            >
                                <option value="github">"GitHub"</option>
                                <option value="gitlab">"GitLab"</option>
                                <option value="gitea">"Gitea"</option>
                                <option value="forgejo">"Forgejo"</option>
                                <option value="custom">"Custom"</option>
                            </select>
                            <input
                                type="text"
                                placeholder="project (org/repo)"
                                class="rounded-md border border-slate-700 bg-slate-900 px-2 py-1 text-xs text-slate-200"
                                prop:value=move || bind_project.get()
                                on:input=move |ev| bind_project.set(event_target_value(&ev))
                                disabled=any_pending
                            />
                            <input
                                type="text"
                                placeholder="item key (e.g. 123)"
                                class="rounded-md border border-slate-700 bg-slate-900 px-2 py-1 text-xs text-slate-200"
                                prop:value=move || bind_item_key.get()
                                on:input=move |ev| bind_item_key.set(event_target_value(&ev))
                                disabled=any_pending
                            />
                            <input
                                type="text"
                                placeholder="url (optional)"
                                class="rounded-md border border-slate-700 bg-slate-900 px-2 py-1 text-xs text-slate-200"
                                prop:value=move || bind_url.get()
                                on:input=move |ev| bind_url.set(event_target_value(&ev))
                                disabled=any_pending
                            />
                        </div>
                        <div class="mt-2 flex gap-2">
                            <button
                                type="button"
                                class="rounded-md border border-teal-400/40 bg-teal-500/10 px-3 py-1 text-xs text-teal-200 hover:bg-teal-500/20 disabled:opacity-50"
                                disabled=any_pending
                                on:click={
                                    let bind_id = bind_id.clone();
                                    move |_| {
                                        let project = bind_project.get().trim().to_string();
                                        let item_key = bind_item_key.get().trim().to_string();
                                        if project.is_empty() || item_key.is_empty() {
                                            return;
                                        }
                                        let url_raw = bind_url.get().trim().to_string();
                                        let url = if url_raw.is_empty() { None } else { Some(url_raw) };
                                        let Some(system) = parse_system_value(&bind_system.get()) else {
                                            return;
                                        };
                                        let binding = ExternalWorkItemBinding {
                                            system,
                                            project,
                                            item_key,
                                            provider_id: None,
                                            url,
                                        };
                                        bind_action.dispatch((bind_id.clone(), Some(binding)));
                                    }
                                }
                            >
                                "Save binding"
                            </button>
                            <button
                                type="button"
                                class="rounded-md border border-rose-400/30 bg-rose-500/10 px-3 py-1 text-xs text-rose-200 hover:bg-rose-500/20 disabled:opacity-50"
                                disabled=any_pending
                                on:click={
                                    let unbind_id = unbind_id.clone();
                                    move |_| {
                                        bind_action.dispatch((unbind_id.clone(), None));
                                    }
                                }
                            >
                                "Unbind"
                            </button>
                        </div>
                    </Show>
                </div>

                {move || any_error().map(|err| view! {
                    <div class="rounded-md border border-rose-400/30 bg-rose-500/10 px-3 py-2 text-[11px] text-rose-200">
                        {err}
                    </div>
                })}
                {move || any_notice().map(|notice| view! {
                    <div class="rounded-md border border-teal-400/30 bg-teal-500/10 px-3 py-2 text-[11px] text-teal-200">
                        {notice}
                    </div>
                })}
            </div>
        </details>
    }
}

fn work_item_state_value(state: &WorkItemState) -> &'static str {
    match state {
        WorkItemState::Backlog => "backlog",
        WorkItemState::Todo => "todo",
        WorkItemState::InProgress => "in-progress",
        WorkItemState::Blocked => "blocked",
        WorkItemState::Done => "done",
        WorkItemState::Cancelled => "cancelled",
    }
}

fn parse_state_value(raw: &str) -> Option<WorkItemState> {
    Some(match raw {
        "backlog" => WorkItemState::Backlog,
        "todo" => WorkItemState::Todo,
        "in-progress" => WorkItemState::InProgress,
        "blocked" => WorkItemState::Blocked,
        "done" => WorkItemState::Done,
        "cancelled" => WorkItemState::Cancelled,
        _ => return None,
    })
}

fn external_system_value(system: &ExternalWorkItemSystem) -> &'static str {
    match system {
        ExternalWorkItemSystem::Github => "github",
        ExternalWorkItemSystem::Gitlab => "gitlab",
        ExternalWorkItemSystem::Gitea => "gitea",
        ExternalWorkItemSystem::Forgejo => "forgejo",
        ExternalWorkItemSystem::Custom => "custom",
    }
}

fn parse_system_value(raw: &str) -> Option<ExternalWorkItemSystem> {
    Some(match raw {
        "github" => ExternalWorkItemSystem::Github,
        "gitlab" => ExternalWorkItemSystem::Gitlab,
        "gitea" => ExternalWorkItemSystem::Gitea,
        "forgejo" => ExternalWorkItemSystem::Forgejo,
        "custom" => ExternalWorkItemSystem::Custom,
        _ => return None,
    })
}

fn work_item_state_label(state: &WorkItemState) -> &'static str {
    match state {
        WorkItemState::Backlog => "Backlog",
        WorkItemState::Todo => "Todo",
        WorkItemState::InProgress => "In Progress",
        WorkItemState::Blocked => "Blocked",
        WorkItemState::Done => "Done",
        WorkItemState::Cancelled => "Cancelled",
    }
}

fn work_item_state_class(state: &WorkItemState) -> &'static str {
    match state {
        WorkItemState::Done => {
            "rounded-full border border-emerald-400/30 bg-emerald-500/10 px-2.5 py-1 text-[10px] font-semibold uppercase tracking-widest text-emerald-200"
        }
        WorkItemState::Blocked => {
            "rounded-full border border-rose-400/30 bg-rose-500/10 px-2.5 py-1 text-[10px] font-semibold uppercase tracking-widest text-rose-200"
        }
        WorkItemState::InProgress => {
            "rounded-full border border-amber-400/30 bg-amber-500/10 px-2.5 py-1 text-[10px] font-semibold uppercase tracking-widest text-amber-100"
        }
        _ => {
            "rounded-full border border-sky-400/30 bg-sky-500/10 px-2.5 py-1 text-[10px] font-semibold uppercase tracking-widest text-sky-200"
        }
    }
}

fn work_item_system_label(system: &WorkItemSystemOfRecord) -> &'static str {
    match system {
        WorkItemSystemOfRecord::Brain => "Brain source",
        WorkItemSystemOfRecord::External => "External source",
        WorkItemSystemOfRecord::Split => "Split source",
    }
}

fn work_item_source_class(system: &WorkItemSystemOfRecord) -> &'static str {
    match system {
        WorkItemSystemOfRecord::Brain => {
            "rounded-full border border-teal-400/30 bg-teal-500/10 px-2.5 py-1 text-[10px] font-semibold uppercase tracking-widest text-teal-200"
        }
        WorkItemSystemOfRecord::External => {
            "rounded-full border border-sky-400/30 bg-sky-500/10 px-2.5 py-1 text-[10px] font-semibold uppercase tracking-widest text-sky-200"
        }
        WorkItemSystemOfRecord::Split => {
            "rounded-full border border-amber-400/30 bg-amber-500/10 px-2.5 py-1 text-[10px] font-semibold uppercase tracking-widest text-amber-100"
        }
    }
}

fn external_system_label(system: &ExternalWorkItemSystem) -> &'static str {
    match system {
        ExternalWorkItemSystem::Github => "GitHub",
        ExternalWorkItemSystem::Gitlab => "GitLab",
        ExternalWorkItemSystem::Gitea => "Gitea",
        ExternalWorkItemSystem::Forgejo => "Forgejo",
        ExternalWorkItemSystem::Custom => "Custom",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use brain_domain::{BrainConfig, NodeTypeSpec};

    use super::*;

    fn spec(name: &str, label: &str) -> NodeTypeSpec {
        NodeTypeSpec {
            name: name.to_string(),
            label: label.to_string(),
            directory: format!("{name}s"),
            accent: "#112233".to_string(),
            template_filename: None,
            creatable: true,
            frontmatter_seed: BTreeMap::new(),
            title_key: None,
            date_create_field: None,
            date_update_field: None,
            body_label: None,
            work_item_kind: None,
            link_fields: BTreeMap::new(),
        }
    }

    fn test_config() -> BrainConfig {
        BrainConfig {
            node_types: vec![spec("note", "Note"), spec("task", "Task")],
            default_type: "note".to_string(),
            label_taxonomy: Vec::new(),
            views: Vec::new(),
        }
    }

    #[test]
    fn group_backlinks_keeps_configured_order_and_sorts_titles() {
        let groups = group_backlinks(
            vec![
                (
                    "task".to_string(),
                    "zeta".to_string(),
                    "tasks/z.md".to_string(),
                ),
                (
                    "note".to_string(),
                    "beta".to_string(),
                    "notes/b.md".to_string(),
                ),
                (
                    "task".to_string(),
                    "alpha".to_string(),
                    "tasks/a.md".to_string(),
                ),
            ],
            &test_config(),
        );

        let labels: Vec<&str> = groups.iter().map(|(spec, _)| spec.label.as_str()).collect();
        assert_eq!(labels, vec!["Note", "Task"]);
        let task_titles: Vec<&str> = groups[1]
            .1
            .iter()
            .map(|(title, _)| title.as_str())
            .collect();
        assert_eq!(task_titles, vec!["alpha", "zeta"]);
    }

    #[test]
    fn group_backlinks_keeps_unknown_types_after_configured_groups() {
        let groups = group_backlinks(
            vec![
                (
                    "mystery".to_string(),
                    "ghost".to_string(),
                    "ghost.md".to_string(),
                ),
                (
                    "task".to_string(),
                    "todo".to_string(),
                    "tasks/t.md".to_string(),
                ),
            ],
            &test_config(),
        );

        let names: Vec<&str> = groups.iter().map(|(spec, _)| spec.name.as_str()).collect();
        let labels: Vec<&str> = groups.iter().map(|(spec, _)| spec.label.as_str()).collect();
        assert_eq!(names, vec!["task", "mystery"]);
        assert_eq!(labels, vec!["Task", "Unknown (mystery)"]);
        assert_eq!(groups[1].1[0].0, "ghost");
    }

    #[test]
    fn group_backlinks_labels_blank_type_as_unknown() {
        let groups = group_backlinks(
            vec![("".to_string(), "untagged".to_string(), "x.md".to_string())],
            &test_config(),
        );

        assert_eq!(groups[0].0.name, "unknown");
        assert_eq!(groups[0].0.label, "Unknown");
    }
}
