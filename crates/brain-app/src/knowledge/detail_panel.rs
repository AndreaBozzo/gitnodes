use leptos::prelude::*;

use super::components::TagBadge;
use super::types::{EditMode, EditPrefill, Node};
#[cfg(not(feature = "ssr"))]
use crate::api::delete_brain_file;
use crate::api::{BrainFile, read_brain_file};

#[component]
pub fn DetailPanel(
    nodes: StoredValue<Vec<Node>>,
    selected: RwSignal<Option<u32>>,
    edit_mode: RwSignal<EditMode>,
    graph_version: RwSignal<u64>,
) -> impl IntoView {
    let current = move || {
        selected
            .get()
            .and_then(|id| nodes.with_value(|ns| ns.iter().find(|n| n.id == id).cloned()))
    };

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

    let loaded_file = move || match file.get() {
        Some(Ok(Some(bf))) => Some(bf),
        _ => None,
    };
    let loaded_sha = move || loaded_file().map(|bf| bf.sha);

    let on_delete = move |path: String, sha: String| {
        #[cfg(not(feature = "ssr"))]
        {
            let confirmed = web_sys::window()
                .and_then(|w| {
                    w.confirm_with_message(&format!(
                        "Delete {path}? This commits to the Brain repo."
                    ))
                    .ok()
                })
                .unwrap_or(false);
            if !confirmed {
                return;
            }
            deleting.set(true);
            delete_error.set(String::new());
            let path_for_task = path.clone();
            leptos::task::spawn_local(async move {
                match delete_brain_file(path_for_task, sha).await {
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
                let github_url = format!(
                    "https://github.com/Dritara-Digital/Brain/blob/main/{}",
                    path
                );
                view! {
                    <aside class="w-[520px] shrink-0 border-l border-slate-800 bg-slate-950 flex flex-col min-h-0">
                        <div class="px-6 py-4 border-b border-slate-800 flex items-start gap-3">
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
                                            class="px-2 py-1 rounded text-[10px] uppercase tracking-widest border border-teal-400/40 text-teal-200 hover:bg-teal-500/10 disabled:opacity-40 disabled:cursor-not-allowed"
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
                                    let path_for_delete = path.clone();
                                    view! {
                                        <button
                                            class="px-2 py-1 rounded text-[10px] uppercase tracking-widest border border-rose-500/40 text-rose-300 hover:bg-rose-500/10 disabled:opacity-40 disabled:cursor-not-allowed"
                                            aria-label="Delete"
                                            disabled=move || deleting.get() || loaded_sha().is_none()
                                            on:click=move |_| {
                                                if let Some(sha) = loaded_sha() {
                                                    on_delete(path_for_delete.clone(), sha);
                                                }
                                            }
                                        >
                                            {move || if deleting.get() { "Deleting…" } else { "Delete" }}
                                        </button>
                                    }
                                }
                                <button
                                    class="text-slate-500 hover:text-slate-200 text-lg leading-none"
                                    aria-label="Close"
                                    on:click=move |_| selected.set(None)
                                >
                                    "×"
                                </button>
                            </div>
                        </div>
                        <Show when=move || !delete_error.get().is_empty()>
                            <div class="px-6 py-2 text-[11px] text-rose-300 border-b border-rose-500/20 bg-rose-500/5">
                                {move || delete_error.get()}
                            </div>
                        </Show>
                        <div class="flex-1 overflow-y-auto px-6 py-5">
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
                                            class="prose prose-invert prose-sm max-w-none"
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
