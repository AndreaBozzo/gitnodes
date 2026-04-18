use std::collections::HashSet;

use leptos::prelude::*;

use super::data;
use super::detail_bar::DetailBar;
use super::editor::EditorPanel;
use super::filter_panel::FilterPanel;
use super::graph_canvas::GraphCanvas;
use super::types::NodeType;

#[component]
pub fn KnowledgePage() -> impl IntoView {
    let nodes = StoredValue::new(data::nodes());
    let edges = StoredValue::new(data::edges());

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
    let editing = RwSignal::new(false);

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
                <div class="ml-auto flex items-center gap-2">
                    {stats.into_iter().map(|(t, count)| view! {
                        <span class="flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-slate-900/80 border border-slate-800 text-[10px] uppercase tracking-widest text-slate-400">
                            <span class="inline-block w-1.5 h-1.5 rounded-full" style=format!("background:{}", t.accent())></span>
                            <span class="text-slate-200 font-semibold">{count}</span>
                            <span>{t.label()}</span>
                        </span>
                    }).collect_view()}
                    <button
                        class="ml-4 px-3 py-1.5 rounded-md bg-teal-500/20 border border-teal-400/40 text-teal-200 text-xs font-medium hover:bg-teal-500/30 transition-colors"
                        on:click=move |_| editing.set(!editing.get_untracked())
                    >
                        {move || if editing.get() { "Close Editor" } else { "+ New" }}
                    </button>
                </div>
            </header>
            <div class="flex-1 flex min-h-0">
                <FilterPanel
                    all_tags=all_tags
                    active_tags=active_tags
                    active_types=active_types
                />
                <Show when=move || editing.get()>
                    <EditorPanel node_titles=node_titles.clone() />
                </Show>
                <GraphCanvas
                    nodes=nodes
                    edges=edges
                    visible_ids=visible_ids.into()
                    hovered=hovered
                    selected=selected
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
