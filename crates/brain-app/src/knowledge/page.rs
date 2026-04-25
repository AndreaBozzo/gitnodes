use std::collections::{HashMap, HashSet};

use leptos::prelude::*;
use leptos_router::hooks::use_query_map;

use super::detail_bar::DetailBar;
use super::detail_panel::DetailPanel;
use super::editor::EditorPanel;
use super::filter_panel::FilterPanel;
use super::graph_canvas::GraphCanvas;
use super::live_sync::LiveSync;
use super::orphan_banner::OrphanBanner;
use super::types::{Edge, EditMode, Node};
use crate::api::{load_brain_config, load_brain_graph, refresh_brain_graph};

#[component]
pub fn KnowledgePage() -> impl IntoView {
    let graph_version = RwSignal::new(0u64);
    let graph = Resource::new_blocking(
        move || graph_version.get(),
        |_| async { load_brain_graph().await },
    );

    // Key the config Resource on `graph_version` too. Without this, the
    // refresh button (and any future webhook) would invalidate the server
    // cache and re-fetch the graph against the new config while the UI's
    // type metadata, filter panel, and orphan banner stay frozen on the old
    // config until a full page reload.
    let config = Resource::new_blocking(
        move || graph_version.get(),
        |_| async { load_brain_config().await },
    );

    view! {
        <Suspense fallback=|| view! {
            <div class="min-h-screen flex items-center justify-center bg-slate-950 text-slate-400 text-sm">
                "Loading knowledge graph…"
            </div>
        }>
            {move || {
                let g = graph.get();
                let c = config.get();
                match (g, c) {
                    (Some(Ok((nodes, edges))), Some(Ok(cfg))) => {
                        KnowledgeView(KnowledgeViewProps { nodes, edges, config: cfg, graph_version }).into_any()
                    }
                    (Some(Err(e)), _) | (_, Some(Err(e))) => view! {
                        <div class="min-h-screen flex items-center justify-center bg-slate-950 text-rose-300 text-sm">
                            {format!("Failed to load graph/config: {e}")}
                        </div>
                    }.into_any(),
                    _ => view! { <div></div> }.into_any(),
                }
            }}
        </Suspense>
    }
}

#[component]
fn KnowledgeView(
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    config: brain_domain::BrainConfig,
    graph_version: RwSignal<u64>,
) -> impl IntoView {
    let query = use_query_map();
    let nodes = StoredValue::new(nodes);
    let config = StoredValue::new(config);
    let edges = StoredValue::new(edges);
    let path_to_id: StoredValue<HashMap<String, u32>> = StoredValue::new(
        nodes.with_value(|ns| ns.iter().map(|n| (n.path.clone(), n.id)).collect()),
    );

    // Tag filtering is case-insensitive: collapse case variants into one
    // lowercase canonical form both in the filter vocabulary and when
    // matching against a node's tags.
    let all_tags: Vec<String> = {
        let mut set: HashSet<String> = HashSet::new();
        nodes.with_value(|ns| {
            for n in ns {
                for t in &n.tags {
                    set.insert(t.to_lowercase());
                }
            }
        });
        let mut v: Vec<String> = set.into_iter().collect();
        v.sort();
        v
    };

    let stats: Vec<(String, usize)> = config
        .with_value(|c| c.node_types.clone())
        .iter()
        .map(|spec| {
            let count =
                nodes.with_value(|ns| ns.iter().filter(|n| n.node_type == spec.name).count());
            (spec.name.clone(), count)
        })
        .collect();

    let active_tags = RwSignal::new(HashSet::<String>::new());
    let active_types = RwSignal::new(HashSet::<String>::new());
    let hovered = RwSignal::new(None::<u32>);
    let selected = RwSignal::new(None::<u32>);
    let edit_mode = RwSignal::new(EditMode::Closed);
    let editing = Memo::new(move |_| !matches!(edit_mode.get(), EditMode::Closed));

    Effect::new(move |_| {
        let params = query.get();
        let Some(path) = params.get_str("path") else {
            return;
        };
        let next = path_to_id.with_value(|map| map.get(path).copied());
        if next != selected.get_untracked() {
            selected.set(next);
        }
    });

    let visible_ids = Memo::new(move |_| {
        let tags = active_tags.get();
        let types = active_types.get();
        nodes.with_value(|ns| {
            ns.iter()
                .filter(|n| types.is_empty() || types.contains(&n.node_type))
                .filter(|n| {
                    tags.is_empty() || n.tags.iter().any(|t| tags.contains(&t.to_lowercase()))
                })
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
        <div class="h-screen flex flex-col bg-slate-950 text-slate-100">
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
                    {
                        let config = config.get_value();
                        let stats_views = stats.into_iter().map(move |(t_name, count)| {
                            let spec = config.lookup(&t_name).unwrap_or(config.default_spec());
                            view! {
                            <span class="flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-slate-900/80 border border-slate-800 text-[10px] uppercase tracking-widest text-slate-400">
                                <span class="inline-block w-1.5 h-1.5 rounded-full" style=format!("background:{}", spec.accent_var())></span>
                                <span class="text-slate-200 font-semibold">{count}</span>
                                <span>{spec.label.clone()}</span>
                            </span>
                            }
                        }).collect::<Vec<_>>();
                        stats_views.into_view()
                    }
                    <LiveSync graph_version=graph_version />
                    <RefreshButton graph_version=graph_version />
                    <button
                        class="ml-2 px-3 py-1.5 rounded-md bg-teal-500/20 border border-teal-400/40 text-teal-200 text-xs font-medium hover:bg-teal-500/30 transition-colors"
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
            <OrphanBanner nodes=nodes config=config />
            <div class="flex-1 flex min-h-0">
                <FilterPanel
                    all_tags=all_tags.clone()
                    active_tags=active_tags
                    active_types=active_types
                    config=config.get_value()
                />
                <Show when=move || editing.get()>
                    <EditorPanel
                        node_titles=node_titles.clone()
                        all_tags=all_tags.clone()
                        edit_mode=edit_mode
                        graph_version=graph_version
                        config=config.get_value()
                    />
                </Show>
                <GraphCanvas
                    nodes=nodes
                    edges=edges
                    visible_ids=visible_ids.into()
                    hovered=hovered
                    selected=selected
                    config=config.get_value()
                />
                <DetailPanel
                    nodes=nodes
                    edges=edges
                    selected=selected
                    edit_mode=edit_mode
                    graph_version=graph_version
                    config=config.get_value()
                />
            </div>
            <DetailBar
                nodes=nodes
                edges=edges
                hovered=hovered.into()
                selected=selected.into()
                config=config.get_value()
            />
        </div>
    }
}

/// Rebuilds the server-side per-target SQLite projection and bumps
/// `graph_version` so the `Resource` re-reads the refreshed snapshot.
#[component]
fn RefreshButton(graph_version: RwSignal<u64>) -> impl IntoView {
    let busy = RwSignal::new(false);
    view! {
        <button
            class="px-3 py-1.5 rounded-md bg-slate-800/60 border border-slate-700 text-slate-300 text-xs font-medium hover:bg-slate-700/70 hover:text-slate-100 transition-colors disabled:opacity-50 disabled:cursor-wait"
            title="Rebuild the local graph projection from the repo."
            disabled=move || busy.get()
            on:click=move |_| {
                if busy.get_untracked() {
                    return;
                }
                busy.set(true);
                leptos::task::spawn_local(async move {
                    let _ = refresh_brain_graph().await;
                    graph_version.update(|v| *v += 1);
                    busy.set(false);
                });
            }
        >
            {move || if busy.get() { "Refreshing…" } else { "Refresh" }}
        </button>
    }
}
