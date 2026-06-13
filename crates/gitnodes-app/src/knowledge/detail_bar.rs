use leptos::prelude::*;

use super::components::TagBadge;
use super::types::{Edge, Node};

const DETAIL_BAR_HEIGHT_PX: usize = 116;
const LINK_PREVIEW_LIMIT: usize = 3;

fn summarize_links(links: &[String]) -> String {
    let link_count = links.len();
    if link_count == 0 {
        return "no links yet".to_string();
    }

    let preview = links
        .iter()
        .take(LINK_PREVIEW_LIMIT)
        .cloned()
        .collect::<Vec<_>>()
        .join(" · ");

    if link_count > LINK_PREVIEW_LIMIT {
        format!(
            "linked to {link_count} nodes · {preview} · +{} more",
            link_count - LINK_PREVIEW_LIMIT
        )
    } else {
        format!(
            "linked to {link_count} node{} · {preview}",
            if link_count == 1 { "" } else { "s" }
        )
    }
}

#[component]
pub fn DetailBar(
    nodes: StoredValue<Vec<Node>>,
    edges: StoredValue<Vec<Edge>>,
    hovered: Signal<Option<u32>>,
    selected: Signal<Option<u32>>,
    config: gitnodes_domain::BrainConfig,
) -> impl IntoView {
    let current_id = Memo::new(move |_| selected.get().or_else(|| hovered.get()));

    let body = move || {
        match current_id.get() {
        None => view! {
            <div class="flex h-full items-center gap-3 overflow-hidden text-xs text-slate-500">
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
            let link_summary = summarize_links(&links);
            let spec = config.lookup(&n.node_type).unwrap_or_else(|| config.default_spec());
            let accent = spec.accent_var();
            let label = spec.label.clone();
            let title = n.title.clone();
            let summary = n.summary.clone();
            let tags = n.tags.clone();
            let tag_count = tags.len();
            view! {
                <div class="flex h-full items-start gap-4 overflow-hidden">
                    <div class="mt-2 h-2 w-2 shrink-0 rounded-full" style=format!("background:{}", accent)></div>
                    <div class="flex min-w-0 flex-1 flex-col overflow-hidden">
                        <div class="flex min-w-0 items-center gap-3 overflow-hidden">
                            <span class="shrink-0 text-[10px] uppercase tracking-widest text-slate-500">{label}</span>
                            <h3 class="min-w-0 flex-1 truncate text-sm font-semibold text-slate-100">{title}</h3>
                            {(tag_count > 0).then(|| {
                                view! {
                                    <div class="flex shrink-0 gap-1 overflow-hidden" style="max-width: 38%;">
                                        {tags.iter().take(3).map(|t| {
                                            view! { <TagBadge tag=t.clone() /> }
                                        }).collect_view()}
                                        {(tag_count > 3).then(|| {
                                            view! {
                                                <span class="self-center text-[10px] uppercase tracking-widest text-slate-500">
                                                    {format!("+{}", tag_count - 3)}
                                                </span>
                                            }
                                        })}
                                    </div>
                                }
                            })}
                        </div>
                        <p class="mt-1 line-clamp-2 break-words text-[12px] leading-relaxed text-slate-400">{summary}</p>
                        <div class="mt-2 truncate text-[11px] text-slate-500">{link_summary}</div>
                    </div>
                </div>
            }
            .into_any()
        }),
    }
    };

    view! {
        <footer
            class="shrink-0 overflow-hidden border-t border-slate-800 bg-slate-900/60 px-6 py-4 backdrop-blur"
            style=format!("height: {DETAIL_BAR_HEIGHT_PX}px;")
        >
            {body}
        </footer>
    }
}

#[cfg(test)]
mod tests {
    use super::summarize_links;

    #[test]
    fn summarize_links_handles_empty_state() {
        assert_eq!(summarize_links(&[]), "no links yet");
    }

    #[test]
    fn summarize_links_caps_preview_length() {
        let links = vec![
            "A".to_string(),
            "B".to_string(),
            "C".to_string(),
            "D".to_string(),
            "E".to_string(),
        ];

        assert_eq!(
            summarize_links(&links),
            "linked to 5 nodes · A · B · C · +2 more"
        );
    }
}
