use std::collections::HashSet;

use leptos::prelude::*;

use super::types::NodeType;

#[component]
pub fn FilterPanel(
    all_tags: Vec<String>,
    active_tags: RwSignal<HashSet<String>>,
    active_types: RwSignal<HashSet<NodeType>>,
) -> impl IntoView {
    let type_buttons = NodeType::ALL
        .iter()
        .map(|t| {
            let t = *t;
            let is_on = Memo::new(move |_| active_types.with(|s| s.contains(&t)));
            let toggle = move |_| {
                active_types.update(|s| {
                    if !s.remove(&t) {
                        s.insert(t);
                    }
                });
            };
            view! {
                <button
                    class="px-3 py-1 rounded-full text-xs border transition-colors flex items-center gap-2"
                    class=("bg-slate-100", move || is_on.get())
                    class=("text-slate-900", move || is_on.get())
                    class=("border-slate-100", move || is_on.get())
                    class=("text-slate-300", move || !is_on.get())
                    class=("border-slate-700", move || !is_on.get())
                    class=("hover:border-slate-500", move || !is_on.get())
                    on:click=toggle
                >
                    <span class="inline-block w-2 h-2 rounded-full" style=format!("background:{}", t.accent_var())></span>
                    {t.label()}
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
                    class="px-2.5 py-1 rounded-md text-[11px] font-medium border transition-colors"
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
    let show_folder_form = RwSignal::new(false);
    let folder_name = RwSignal::new(String::new());
    let parent_folder = RwSignal::new(String::new());
    let folder_status = RwSignal::new(String::new());
    let folder_saving = RwSignal::new(false);

    let on_create_folder = move |_| {
        let name = folder_name.get_untracked().trim().to_string();
        if name.is_empty() {
            folder_status.set("Folder name is required".to_string());
            return;
        }

        folder_saving.set(true);
        folder_status.set("Creating…".to_string());

        let _full_path = {
            let parent = parent_folder
                .get_untracked()
                .trim()
                .trim_matches('/')
                .to_string();
            if parent.is_empty() {
                name.clone()
            } else {
                format!("{parent}/{name}")
            }
        };

        #[cfg(not(feature = "ssr"))]
        {
            use crate::api::create_folder;
            leptos::task::spawn_local(async move {
                match create_folder(_full_path).await {
                    Ok(path) => {
                        folder_status.set(format!("Created: {path}"));
                        folder_name.set(String::new());
                        parent_folder.set(String::new());
                    }
                    Err(e) => {
                        folder_status.set(format!("Error: {e}"));
                    }
                }
                folder_saving.set(false);
            });
        }
    };

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

            // --- New Folder / Section ---
            <section class="pt-4 border-t border-slate-800">
                <button
                    class="w-full text-left text-[10px] font-semibold tracking-widest uppercase text-teal-400 hover:text-teal-300 transition-colors mb-2"
                    on:click=move |_| show_folder_form.update(|v| *v = !*v)
                >
                    {move || if show_folder_form.get() { "▾ New Section" } else { "▸ New Section" }}
                </button>
                <Show when=move || show_folder_form.get()>
                    <div class="space-y-2">
                        <input
                            type="text"
                            class="w-full px-2 py-1.5 rounded-md bg-slate-800 border border-slate-700 text-slate-100 text-xs focus:border-teal-400 focus:outline-none"
                            placeholder="Parent (e.g. concepts) — optional"
                            prop:value=move || parent_folder.get()
                            on:input=move |ev| parent_folder.set(event_target_value(&ev))
                        />
                        <input
                            type="text"
                            class="w-full px-2 py-1.5 rounded-md bg-slate-800 border border-slate-700 text-slate-100 text-xs focus:border-teal-400 focus:outline-none"
                            placeholder="Folder name"
                            prop:value=move || folder_name.get()
                            on:input=move |ev| folder_name.set(event_target_value(&ev))
                        />
                        <button
                            class="w-full px-3 py-1.5 rounded-md bg-teal-500/20 border border-teal-400/40 text-teal-200 text-xs font-medium hover:bg-teal-500/30 transition-colors disabled:opacity-50"
                            disabled=move || folder_saving.get()
                            on:click=on_create_folder
                        >
                            {move || if folder_saving.get() { "Creating…" } else { "Create Folder" }}
                        </button>
                        <Show when=move || !folder_status.get().is_empty()>
                            <p class="text-[10px] text-slate-400">{move || folder_status.get()}</p>
                        </Show>
                    </div>
                </Show>
            </section>

            <section class="text-[11px] text-slate-500 leading-relaxed pt-4 border-t border-slate-800">
                <p>"Empty filter means everything visible. Hover a node to emphasise its neighbourhood; click to lock it into the detail bar."</p>
            </section>
        </aside>
    }
}
