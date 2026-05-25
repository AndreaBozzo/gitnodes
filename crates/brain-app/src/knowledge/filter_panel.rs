use std::collections::{HashMap, HashSet};

use leptos::prelude::*;

use crate::api::RepoFile;
use crate::knowledge::brain_switcher::BrainSwitcher;
use crate::knowledge::repo_structure::RepoStructureTree;

#[component]
pub fn FilterPanel(
    all_tags: Vec<String>,
    active_tags: RwSignal<HashSet<String>>,
    active_types: RwSignal<HashSet<String>>,
    active_path_prefix: RwSignal<Option<String>>,
    active_orphan_filter: RwSignal<bool>,
    selected_path: RwSignal<Option<String>>,
    repo_files: Vec<RepoFile>,
    config: brain_domain::BrainConfig,
    type_counts: HashMap<String, usize>,
    #[prop(optional)] current_org: String,
    #[prop(optional)] current_repo: String,
    #[prop(optional)] current_branch: String,
) -> impl IntoView {
    let type_buttons = config.node_types
        .iter()
        .map(|spec| {
            let t = spec.name.clone();
            let count = type_counts.get(&t).copied().unwrap_or(0);
            let is_on = Memo::new({
                let t = t.clone();
                move |_| active_types.with(|s| s.contains(&t))
            });
            let toggle = {
                let t = t.clone();
                move |_| {
                    active_types.update(|s| {
                        if !s.remove(&t) {
                            s.insert(t.clone());
                        }
                    });
                }
            };
            view! {
                <button
                    class="px-3 py-1 rounded-full text-xs border transition-colors flex items-center gap-2 focus:outline-none focus:ring-1 focus:ring-slate-500"
                    class=("bg-slate-100", move || is_on.get())
                    class=("text-slate-900", move || is_on.get())
                    class=("border-slate-100", move || is_on.get())
                    class=("text-slate-300", move || !is_on.get())
                    class=("border-slate-700", move || !is_on.get())
                    class=("hover:border-slate-500", move || !is_on.get())
                    class=("opacity-50", move || count == 0)
                    on:click=toggle
                >
                    <span class="inline-block w-2 h-2 rounded-full" style=format!("background:{}", spec.accent_var())></span>
                    <span>{spec.label.clone()}</span>
                    <span class="text-[10px] tabular-nums opacity-70">{count}</span>
                </button>
            }
        })
        .collect_view();

    let view_buttons = config
        .sorted_views()
        .into_iter()
        .map(|v| {
            let view_tags: HashSet<String> = v.tags.iter().cloned().collect();
            let view_types: HashSet<String> = v.types.iter().cloned().collect();
            let label = v.name.clone();
            let is_on = {
                let view_tags = view_tags.clone();
                let view_types = view_types.clone();
                Memo::new(move |_| {
                    active_tags.with(|t| t == &view_tags)
                        && active_types.with(|t| t == &view_types)
                })
            };
            let apply = move |_| {
                if is_on.get() {
                    active_tags.update(|s| s.clear());
                    active_types.update(|s| s.clear());
                } else {
                    active_tags.set(view_tags.clone());
                    active_types.set(view_types.clone());
                }
            };
            view! {
                <button
                    class="px-2.5 py-1 rounded-md text-[11px] font-medium border transition-colors focus:outline-none focus:ring-1 focus:ring-slate-500"
                    class=("bg-amber-400/20", move || is_on.get())
                    class=("text-amber-100", move || is_on.get())
                    class=("border-amber-400/60", move || is_on.get())
                    class=("text-slate-300", move || !is_on.get())
                    class=("border-slate-700", move || !is_on.get())
                    class=("hover:border-slate-500", move || !is_on.get())
                    on:click=apply
                    title=format!(
                        "tags: {}\ntypes: {}",
                        if v.tags.is_empty() { "—".to_string() } else { v.tags.join(", ") },
                        if v.types.is_empty() { "—".to_string() } else { v.types.join(", ") },
                    )
                >
                    {label}
                </button>
            }
        })
        .collect_view();
    let has_views = !config.views.is_empty();

    let all_tags_total = all_tags.len();
    let all_tags_stored = StoredValue::new(all_tags);
    let tag_filter = RwSignal::new(String::new());

    let visible_tags = Memo::new(move |_| {
        let needle = tag_filter.get().to_lowercase();
        let active = active_tags.get();
        let mut pinned: Vec<String> = Vec::new();
        let mut rest: Vec<String> = Vec::new();
        all_tags_stored.with_value(|tags| {
            for tag in tags {
                let matches = needle.is_empty() || tag.contains(&needle);
                if !matches {
                    continue;
                }
                if active.contains(tag) {
                    pinned.push(tag.clone());
                } else {
                    rest.push(tag.clone());
                }
            }
        });
        pinned.append(&mut rest);
        pinned
    });

    let render_tag_button = move |tag: String| {
        let tag_cmp = tag.clone();
        let is_on = Memo::new(move |_| active_tags.with(|s| s.contains(&tag_cmp)));
        let tag_toggle = tag.clone();
        let toggle = move |_| {
            let t = tag_toggle.clone();
            active_tags.update(|s| {
                if !s.remove(&t) {
                    s.insert(t);
                }
            });
        };
        view! {
            <button
                class="px-2.5 py-1 rounded-md text-[11px] font-medium border transition-colors focus:outline-none focus:ring-1 focus:ring-slate-500"
                class=("bg-teal-400/20", move || is_on.get())
                class=("text-teal-200", move || is_on.get())
                class=("border-teal-400/60", move || is_on.get())
                class=("text-slate-400", move || !is_on.get())
                class=("border-slate-700", move || !is_on.get())
                class=("hover:border-slate-500", move || !is_on.get())
                on:click=toggle
            >
                {"#"}{tag}
            </button>
        }
    };

    // --- Folder creation state ---
    // (Removed in favor of implicit folders)

    let any_filter_active = Memo::new(move |_| {
        active_tags.with(|t| !t.is_empty())
            || active_types.with(|t| !t.is_empty())
            || active_path_prefix.with(|p| p.is_some())
            || active_orphan_filter.get()
    });
    let scope_label = Memo::new(move |_| {
        let mut parts: Vec<String> = Vec::new();
        let type_count = active_types.with(HashSet::len);
        let tag_count = active_tags.with(HashSet::len);
        if type_count > 0 {
            parts.push(format!(
                "{type_count} type{}",
                if type_count == 1 { "" } else { "s" }
            ));
        }
        if tag_count > 0 {
            parts.push(format!(
                "{tag_count} tag{}",
                if tag_count == 1 { "" } else { "s" }
            ));
        }
        if let Some(prefix) = active_path_prefix.get() {
            parts.push(format!("path {}", prefix.trim_end_matches('/')));
        }
        if active_orphan_filter.get() {
            parts.push("isolated files".to_string());
        }
        if parts.is_empty() {
            "All nodes visible".to_string()
        } else {
            parts.join(" + ")
        }
    });
    let clear_all = move |_| {
        active_tags.update(|s| s.clear());
        active_types.update(|s| s.clear());
        active_path_prefix.set(None);
        active_orphan_filter.set(false);
    };

    // Where the "clear" button lives depends on whether the Views row exists.
    // We want at most one rendered, anchored to the topmost section header so
    // the user always sees it without scrolling.
    let clear_in_views_row = move || {
        any_filter_active.get().then(|| view! {
            <button
                class="ml-auto text-[10px] uppercase tracking-widest text-slate-500 hover:text-slate-200"
                on:click=clear_all
                title="Clear all filters"
            >
                "clear"
            </button>
        })
    };
    let clear_in_type_row = move || {
        (!has_views && any_filter_active.get()).then(|| view! {
            <button
                class="ml-auto text-[10px] uppercase tracking-widest text-slate-500 hover:text-slate-200"
                on:click=clear_all
                title="Clear all filters"
            >
                "clear"
            </button>
        })
    };

    // Reserved bottom slot for 4.x time-history graph. Hidden today; the
    // filter pane reclaims the full height. Flip `HISTORY_SLOT_ENABLED` (or
    // gate via cargo feature) when the time-history surface lands.
    const HISTORY_SLOT_ENABLED: bool = false;
    let has_tags = all_tags_total > 0;
    let no_tags = all_tags_total == 0;

    view! {
        <aside class="w-64 shrink-0 border-r border-slate-800 bg-slate-900/40 flex flex-col h-full min-h-0">
            <div class="px-5 pt-5 pb-3 shrink-0 border-b border-slate-800/60">
                <BrainSwitcher
                    current_org=(!current_org.is_empty()).then(|| current_org.clone())
                    current_repo=(!current_repo.is_empty()).then(|| current_repo.clone())
                    current_branch=(!current_branch.is_empty()).then(|| current_branch.clone())
                />
            </div>
            <div class="flex-1 min-h-0 overflow-y-auto px-5 py-4 space-y-6">
                {has_views.then(|| view! {
                    <section>
                        <div class="flex items-center mb-3">
                            <h2 class="text-[10px] font-semibold tracking-widest uppercase text-slate-500">"Views"</h2>
                            {clear_in_views_row}
                        </div>
                        <div class="flex flex-wrap gap-2">{view_buttons}</div>
                    </section>
                })}
                <section>
                    <div class="flex items-center mb-3">
                        <h2 class="text-[10px] font-semibold tracking-widest uppercase text-slate-500">"Types"</h2>
                        {clear_in_type_row}
                    </div>
                    <div class="flex flex-wrap gap-2">{type_buttons}</div>
                </section>
                <RepoStructureTree
                    files=repo_files
                    active_path_prefix=active_path_prefix
                    active_orphan_filter=active_orphan_filter
                    selected_path=selected_path
                    config=config.clone()
                    current_org=current_org.clone()
                    current_repo=current_repo.clone()
                />
                <section>
                    <div class="flex items-center justify-between mb-2">
                        <h2 class="text-[10px] font-semibold tracking-widest uppercase text-slate-500">
                            "Tags"
                            <Show when=move || has_tags>
                                <span class="ml-1.5 text-slate-600 normal-case tracking-normal">{all_tags_total}</span>
                            </Show>
                        </h2>
                    </div>
                    <Show when=move || has_tags>
                        <input
                            type="search"
                            placeholder="Filter tags…"
                            class="w-full px-2 py-1 mb-2 rounded bg-slate-900/60 border border-slate-800 text-[11px] text-slate-200 placeholder:text-slate-600 focus:outline-none focus:border-slate-600"
                            prop:value=move || tag_filter.get()
                            on:input=move |ev| tag_filter.set(event_target_value(&ev))
                        />
                        <div class="max-h-48 overflow-y-auto pr-1">
                            <div class="flex flex-wrap gap-2">
                                {move || visible_tags.get().into_iter().map(render_tag_button).collect_view()}
                            </div>
                            <Show when=move || visible_tags.with(|v| v.is_empty()) && !tag_filter.with(String::is_empty)>
                                <p class="text-[10px] text-slate-600 italic mt-1">"No tags match."</p>
                            </Show>
                        </div>
                    </Show>
                    <Show when=move || no_tags>
                        <p class="text-[10px] text-slate-600 italic">"No tags in this graph."</p>
                    </Show>
                </section>

                <section class="rounded-md border border-slate-800 bg-slate-950/40 px-3 py-2 text-[11px] text-slate-400">
                    <div class="flex items-center gap-2">
                        <span class="h-1.5 w-1.5 rounded-full bg-slate-500"></span>
                        <span class="min-w-0 flex-1 truncate">{move || scope_label.get()}</span>
                    </div>
                </section>
            </div>
            <Show when=move || HISTORY_SLOT_ENABLED>
                <div class="shrink-0 border-t border-slate-800 h-1/3 min-h-[160px] overflow-y-auto px-5 py-3 bg-slate-950/40">
                    <h2 class="text-[10px] font-semibold tracking-widest uppercase text-slate-500 mb-2">"History"</h2>
                    <p class="text-[11px] text-slate-600 italic">"Time-history graph lands in 4.x."</p>
                </div>
            </Show>
        </aside>
    }
}
