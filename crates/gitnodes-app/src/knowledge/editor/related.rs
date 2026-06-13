// Copyright (C) 2026 Andrea Bozzo
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use leptos::prelude::*;

use crate::knowledge::components::RemovableBadge;

/// Searchable node picker for forced "Related / See also" linking.
#[component]
pub(super) fn RelatedLinksPicker(
    selected_related: RwSignal<Vec<String>>,
    node_titles: StoredValue<Vec<(String, String)>>,
) -> impl IntoView {
    let link_search = RwSignal::new(String::new());

    let filtered_nodes = Memo::new(move |_| {
        let query = link_search.get().to_lowercase();
        if query.is_empty() {
            return vec![];
        }
        node_titles.with_value(|nodes| {
            nodes
                .iter()
                .filter(|(_, t)| t.to_lowercase().contains(&query))
                .take(8)
                .cloned()
                .collect::<Vec<_>>()
        })
    });

    view! {
        <div>
            <label class="text-[10px] uppercase tracking-widest text-slate-500 mb-1 block">"Related / See also"</label>
            <input
                type="text"
                class="w-full px-3 py-2 rounded-md bg-slate-800 border border-slate-700 text-slate-100 text-sm focus:border-teal-400 focus:outline-none mb-2"
                placeholder="Search existing nodes…"
                prop:value=move || link_search.get()
                on:input=move |ev| link_search.set(event_target_value(&ev))
            />
            <div class="space-y-1 max-h-32 overflow-y-auto">
                {move || {
                    filtered_nodes.get().into_iter().map(|(path, title)| {
                        let path_clone = path.clone();
                        let already = Memo::new({
                            let path = path.clone();
                            move |_| selected_related.with(|r| r.contains(&path))
                        });
                        view! {
                            <button
                                class="w-full text-left px-2 py-1 rounded text-xs hover:bg-slate-700 transition-colors focus:outline-none focus:ring-1 focus:ring-slate-500"
                                class=("text-teal-300", move || already.get())
                                class=("bg-slate-700/50", move || already.get())
                                class=("text-slate-300", move || !already.get())
                                on:click=move |_| {
                                    let p = path_clone.clone();
                                    selected_related.update(|r| {
                                        if let Some(idx) = r.iter().position(|x| x == &p) {
                                            r.remove(idx);
                                        } else {
                                            r.push(p);
                                        }
                                    });
                                }
                            >
                                {if already.get_untracked() { "✓ " } else { "+ " }}
                                {title}
                            </button>
                        }
                    }).collect_view()
                }}
            </div>
            <div class="flex flex-wrap gap-1 mt-2">
                {move || selected_related.get().into_iter().map(|path| {
                    let path_for_remove = path.clone();
                    let display = path.rsplit('/').next().unwrap_or(&path).replace(".md", "");
                    view! {
                        <RemovableBadge
                            label=display
                            on_remove=move || {
                                let p = path_for_remove.clone();
                                selected_related.update(|r| r.retain(|x| x != &p));
                            }
                        />
                    }
                }).collect_view()}
            </div>
        </div>
    }
}
