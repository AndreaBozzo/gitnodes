use leptos::prelude::*;

use super::components::TagBadge;
use super::types::{Edge, Node};

#[component]
pub fn DetailBar(
    nodes: StoredValue<Vec<Node>>,
    edges: StoredValue<Vec<Edge>>,
    hovered: Signal<Option<u32>>,
    selected: Signal<Option<u32>>,
    config: brain_domain::BrainConfig,
) -> impl IntoView {
    let current_id = Memo::new(move |_| selected.get().or_else(|| hovered.get()));

    let body = move || {
        match current_id.get() {
        None => view! {
            <div class="flex items-center gap-3 text-xs text-slate-500">
                <span class="h-2 w-2 rounded-full bg-slate-700"></span>
                <span>"No node selected"</span>
                <span class="text-slate-700">"/"</span>
                <span>"Graph context will appear here."</span>
            </div>
        }
        .into_any(),
        Some(id) => nodes.with_value(|ns| {
            let Some(n) = ns.iter().find(|n| n.id == id).cloned() else {
                return view! { <div/> }.into_any();
            };
            let links: Vec<String> = edges.with_value(|es| {
                es.iter()
                    .filter_map(|e| {
                        if e.from == id {
                            Some(e.to)
                        } else if e.to == id {
                            Some(e.from)
                        } else {
                            None
                        }
                    })
                    .filter_map(|other| ns.iter().find(|m| m.id == other).map(|m| m.title.clone()))
                    .collect()
            });
            let link_count = links.len();
            let link_summary = if link_count == 0 {
                "no links yet".to_string()
            } else {
                format!(
                    "linked to {link_count} node{} · {}",
                    if link_count == 1 { "" } else { "s" },
                    links.join(" · ")
                )
            };
            let spec = config.lookup(&n.node_type).unwrap_or_else(|| config.default_spec());
            let accent = spec.accent_var();
            let label = spec.label.clone();
            let title = n.title.clone();
            let summary = n.summary.clone();
            let tags = n.tags.clone();
            view! {
                <div class="flex items-start gap-4">
                    <div class="w-2 h-2 rounded-full mt-2" style=format!("background:{}", accent)></div>
                    <div class="flex-1 min-w-0">
                        <div class="flex items-center gap-3 flex-wrap">
                            <span class="text-[10px] uppercase tracking-widest text-slate-500">{label}</span>
                            <h3 class="text-sm font-semibold text-slate-100 truncate">{title}</h3>
                            <div class="flex gap-1">
                                {tags.iter().map(|t| {
                                    view! { <TagBadge tag=t.clone() /> }
                                }).collect_view()}
                            </div>
                        </div>
                        <p class="text-[12px] text-slate-400 mt-1 leading-relaxed">{summary}</p>
                        <div class="text-[11px] text-slate-500 mt-2">{link_summary}</div>
                    </div>
                </div>
            }
            .into_any()
        }),
    }
    };

    view! {
        <footer class="border-t border-slate-800 bg-slate-900/60 backdrop-blur px-6 py-4 min-h-[92px]">
            {body}
        </footer>
    }
}
