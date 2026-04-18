use leptos::prelude::*;

use super::types::Node;
use crate::api::{BrainFile, read_brain_file};

#[component]
pub fn DetailPanel(
    nodes: StoredValue<Vec<Node>>,
    selected: RwSignal<Option<u32>>,
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

    view! {
        <Show when=move || current().map(|n| !n.path.is_empty()).unwrap_or(false)>
            {move || {
                let node = current().expect("guarded by Show");
                let accent = node.node_type.accent().to_string();
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
                                        let t = t.clone();
                                        view! {
                                            <span class="px-2 py-0.5 rounded text-[10px] bg-slate-800 text-slate-300 border border-slate-700">
                                                {"#"}{t}
                                            </span>
                                        }
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
                                    <span class="text-slate-500 truncate">{path}</span>
                                </div>
                            </div>
                            <button
                                class="text-slate-500 hover:text-slate-200 text-lg leading-none shrink-0"
                                aria-label="Close"
                                on:click=move |_| selected.set(None)
                            >
                                "×"
                            </button>
                        </div>
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
