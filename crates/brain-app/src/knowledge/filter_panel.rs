use std::collections::HashSet;

use leptos::prelude::*;

#[component]
pub fn FilterPanel(
    all_tags: Vec<String>,
    active_tags: RwSignal<HashSet<String>>,
    active_types: RwSignal<HashSet<String>>,
    config: brain_domain::BrainConfig,
) -> impl IntoView {
    let type_buttons = config.node_types
        .iter()
        .map(|spec| {
            let t = spec.name.clone();
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
                    on:click=toggle
                >
                    <span class="inline-block w-2 h-2 rounded-full" style=format!("background:{}", spec.accent_var)></span>
                    {spec.label.clone()}
                </button>
            }
        })
        .collect_view();

    let tag_buttons = all_tags
        .into_iter()
        .map(|tag| {
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
        })
        .collect_view();

    // --- Folder creation state ---
    // (Removed in favor of implicit folders)

    view! {
        <aside class="w-64 shrink-0 border-r border-slate-800 bg-slate-900/40 p-5 space-y-6 overflow-y-auto">
            <section>
                <h2 class="text-[10px] font-semibold tracking-widest uppercase text-slate-500 mb-3">"Type"</h2>
                <div class="flex flex-wrap gap-2">{type_buttons}</div>
            </section>
            <section>
                <h2 class="text-[10px] font-semibold tracking-widest uppercase text-slate-500 mb-3">"Tags"</h2>
                <div class="flex flex-wrap gap-2">{tag_buttons}</div>
            </section>



            <section class="text-[11px] text-slate-500 leading-relaxed pt-4 border-t border-slate-800">
                <p>"Empty filter means everything visible. Hover a node to emphasise its neighbourhood; click to lock it into the detail bar."</p>
            </section>
        </aside>
    }
}
