use std::collections::HashSet;

use leptos::prelude::*;

use super::detail_bar::DetailBar;
use super::detail_panel::DetailPanel;
use super::editor::EditorPanel;
use super::filter_panel::FilterPanel;
use super::graph_canvas::GraphCanvas;
use super::types::{Edge, EditMode, Node, NodeType};
use crate::api::load_brain_graph;

#[component]
pub fn KnowledgePage() -> impl IntoView {
    let graph_version = RwSignal::new(0u64);
    let graph = Resource::new_blocking(
        move || graph_version.get(),
        |_| async { load_brain_graph().await },
    );

    view! {
        <Suspense fallback=|| view! {
            <div class="min-h-screen flex items-center justify-center bg-slate-950 text-slate-400 text-sm">
                "Loading knowledge graph…"
            </div>
        }>
            {move || graph.get().map(|res| match res {
                Ok((nodes, edges)) => KnowledgeView(KnowledgeViewProps { nodes, edges, graph_version }).into_any(),
                Err(e) => view! {
                    <div class="min-h-screen flex items-center justify-center bg-slate-950 text-rose-300 text-sm">
                        {format!("Failed to load graph: {e}")}
                    </div>
                }.into_any(),
            })}
        </Suspense>
    }
}

#[component]
fn KnowledgeView(
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    graph_version: RwSignal<u64>,
) -> impl IntoView {
    let nodes = StoredValue::new(nodes);
    let edges = StoredValue::new(edges);

    let all_tags: Vec<String> = {
        let mut set: HashSet<String> = HashSet::new();
        nodes.with_value(|ns| {
            for n in ns {
                for t in &n.tags {
                    set.insert(t.clone());
                }
            }
        });
        let mut v: Vec<String> = set.into_iter().collect();
        v.sort();
        v
    };

    let stats: Vec<(NodeType, usize)> = NodeType::ALL
        .iter()
        .map(|t| {
            let count = nodes.with_value(|ns| ns.iter().filter(|n| n.node_type == *t).count());
            (*t, count)
        })
        .collect();

    let active_tags = RwSignal::new(HashSet::<String>::new());
    let active_types = RwSignal::new(HashSet::<NodeType>::new());
    let hovered = RwSignal::new(None::<u32>);
    let selected = RwSignal::new(None::<u32>);
    let edit_mode = RwSignal::new(EditMode::Closed);
    let editing = Memo::new(move |_| !matches!(edit_mode.get(), EditMode::Closed));

    let visible_ids = Memo::new(move |_| {
        let tags = active_tags.get();
        let types = active_types.get();
        nodes.with_value(|ns| {
            ns.iter()
                .filter(|n| types.is_empty() || types.contains(&n.node_type))
                .filter(|n| tags.is_empty() || n.tags.iter().any(|t| tags.contains(t)))
                .map(|n| n.id)
                .collect::<HashSet<u32>>()
        })
    });

    let node_titles: Vec<(String, String)> = nodes.with_value(|ns| {
        ns.iter()
            .filter(|n| !n.path.is_empty())
            .map(|n| (n.path.clone(), n.title.clone()))
            .collect()
    });

    view! {
        <div class="min-h-screen flex flex-col bg-slate-950 text-slate-100">
            <header class="px-6 py-4 border-b border-slate-800 flex items-center gap-3">
                <div class="w-2 h-2 rounded-full bg-teal-400"></div>
                <h1 class="text-sm font-semibold tracking-wide uppercase text-slate-300">
                    "Brain · Knowledge"
                </h1>
                <span class="text-xs text-slate-500 ml-2">"admin · /knowledge"</span>
                <a
                    href="/admin"
                    class="text-xs text-slate-500 hover:text-slate-300 ml-2"
                >
                    "· /admin"
                </a>
                <div class="ml-auto flex items-center gap-2">
                    {stats.into_iter().map(|(t, count)| view! {
                        <span class="flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-slate-900/80 border border-slate-800 text-[10px] uppercase tracking-widest text-slate-400">
                            <span class="inline-block w-1.5 h-1.5 rounded-full" style=format!("background:{}", t.accent_var())></span>
                            <span class="text-slate-200 font-semibold">{count}</span>
                            <span>{t.label()}</span>
                        </span>
                    }).collect_view()}
                    <button
                        class="ml-4 px-3 py-1.5 rounded-md bg-teal-500/20 border border-teal-400/40 text-teal-200 text-xs font-medium hover:bg-teal-500/30 transition-colors"
                        on:click=move |_| {
                            edit_mode.update(|m| {
                                *m = if matches!(m, EditMode::Closed) {
                                    EditMode::New
                                } else {
                                    EditMode::Closed
                                };
                            });
                        }
                    >
                        {move || if editing.get() { "Close Editor" } else { "+ New" }}
                    </button>
                </div>
            </header>
            <div class="flex-1 flex min-h-0">
                <FilterPanel
                    all_tags=all_tags.clone()
                    active_tags=active_tags
                    active_types=active_types
                />
                <Show when=move || editing.get()>
                    <EditorPanel
                        node_titles=node_titles.clone()
                        all_tags=all_tags.clone()
                        edit_mode=edit_mode
                        graph_version=graph_version
                    />
                </Show>
                <GraphCanvas
                    nodes=nodes
                    edges=edges
                    visible_ids=visible_ids.into()
                    hovered=hovered
                    selected=selected
                />
                <DetailPanel
                    nodes=nodes
                    selected=selected
                    edit_mode=edit_mode
                    graph_version=graph_version
                />
            </div>
            <DetailBar
                nodes=nodes
                edges=edges
                hovered=hovered.into()
                selected=selected.into()
            />
        </div>
    }
}
