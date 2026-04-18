use leptos::prelude::*;

use super::types::NodeType;
use crate::api::get_current_user;

/// Smart editor form that enforces Brain templates programmatically.
/// - Structured fields for title, author, tags (no raw frontmatter editing)
/// - Forced linking via searchable node list
/// - Live markdown preview (client-side pulldown-cmark)
/// - Auto-generates compliant YAML frontmatter on submit
#[component]
pub fn EditorPanel(
    /// Available nodes for the "Related / See also" forced-linking picker.
    node_titles: Vec<(String, String)>,
    /// Existing tag vocabulary across the repo.
    all_tags: Vec<String>,
) -> impl IntoView {
    let node_type = RwSignal::new(NodeType::Concept);
    let title = RwSignal::new(String::new());
    let author = RwSignal::new(String::new());
    let tags = RwSignal::new(Vec::<String>::new());
    let tag_input = RwSignal::new(String::new());
    let body = RwSignal::new(String::new());
    let selected_related = RwSignal::new(Vec::<String>::new());
    let link_search = RwSignal::new(String::new());
    let status_msg = RwSignal::new(String::new());
    let saving = RwSignal::new(false);
    let show_preview = RwSignal::new(false);

    // Auto-fill author from the current GitHub session (once).
    let session_user = Resource::new(|| (), |_| async { get_current_user().await });
    Effect::new(move |_| {
        if let Some(Ok(Some(login))) = session_user.get()
            && author.with_untracked(|a| a.is_empty())
        {
            author.set(login);
        }
    });

    let node_titles_stored = StoredValue::new(node_titles);
    let all_tags_stored = StoredValue::new(all_tags);

    let filtered_nodes = Memo::new(move |_| {
        let query = link_search.get().to_lowercase();
        if query.is_empty() {
            return vec![];
        }
        node_titles_stored.with_value(|nodes| {
            nodes
                .iter()
                .filter(|(_, t)| t.to_lowercase().contains(&query))
                .take(8)
                .cloned()
                .collect::<Vec<_>>()
        })
    });

    let tag_suggestions = Memo::new(move |_| {
        let query = tag_input.get().to_lowercase();
        if query.is_empty() {
            return vec![];
        }
        let current = tags.get();
        all_tags_stored.with_value(|all| {
            all.iter()
                .filter(|t| t.to_lowercase().contains(&query) && !current.contains(t))
                .take(6)
                .cloned()
                .collect::<Vec<_>>()
        })
    });

    let add_tag = move |raw: String| {
        for piece in raw.split(|c: char| c.is_whitespace() || c == ',') {
            let t = piece.trim().trim_start_matches('#').trim().to_string();
            if t.is_empty() {
                continue;
            }
            tags.update(|v| {
                if !v.contains(&t) {
                    v.push(t);
                }
            });
        }
        tag_input.set(String::new());
    };

    let preview_html = Memo::new(move |_| crate::markdown::render(&body.get()));

    let on_submit = move |_| {
        // Flush any unconfirmed text in the tag input (user forgot to press Enter).
        let pending = tag_input.get_untracked();
        if !pending.trim().is_empty() {
            add_tag(pending);
        }
        let _payload = crate::knowledge::types::BrainFilePayload {
            node_type: node_type.get_untracked(),
            title: title.get_untracked(),
            author: author.get_untracked(),
            tags: tags.get_untracked(),
            body: body.get_untracked(),
            related: selected_related.get_untracked(),
            path: None,
            sha: None,
        };

        saving.set(true);
        status_msg.set("Saving…".to_string());

        #[cfg(not(feature = "ssr"))]
        {
            use crate::api::save_brain_file;
            leptos::task::spawn_local(async move {
                match save_brain_file(_payload).await {
                    Ok(path) => {
                        status_msg.set(format!("Created: {path}"));
                        title.set(String::new());
                        body.set(String::new());
                        tags.set(vec![]);
                        selected_related.set(vec![]);
                    }
                    Err(e) => {
                        status_msg.set(format!("Error: {e}"));
                    }
                }
                saving.set(false);
            });
        }
    };

    view! {
        <aside class="w-[420px] shrink-0 border-r border-slate-800 bg-slate-900/60 p-5 space-y-4 overflow-y-auto">
            <h2 class="text-xs font-semibold tracking-widest uppercase text-teal-400 mb-2">
                "New Document"
            </h2>

            <div>
                <label class="text-[10px] uppercase tracking-widest text-slate-500 mb-1 block">"Type"</label>
                <div class="flex gap-2">
                    {[NodeType::Concept, NodeType::Decision, NodeType::Meeting].iter().map(|t| {
                        let t = *t;
                        let is_active = Memo::new(move |_| node_type.get() == t);
                        view! {
                            <button
                                class="px-3 py-1 rounded-full text-xs border transition-colors flex items-center gap-2"
                                class=("bg-slate-100", move || is_active.get())
                                class=("text-slate-900", move || is_active.get())
                                class=("border-slate-100", move || is_active.get())
                                class=("text-slate-300", move || !is_active.get())
                                class=("border-slate-700", move || !is_active.get())
                                on:click=move |_| node_type.set(t)
                            >
                                <span class="inline-block w-2 h-2 rounded-full" style=format!("background:{}", t.accent())></span>
                                {t.label()}
                            </button>
                        }
                    }).collect_view()}
                </div>
            </div>

            <div>
                <label class="text-[10px] uppercase tracking-widest text-slate-500 mb-1 block">"Title / Topic"</label>
                <input
                    type="text"
                    class="w-full px-3 py-2 rounded-md bg-slate-800 border border-slate-700 text-slate-100 text-sm focus:border-teal-400 focus:outline-none"
                    placeholder="e.g. MeetingAutomation"
                    prop:value=move || title.get()
                    on:input=move |ev| title.set(event_target_value(&ev))
                />
            </div>

            <div>
                <label class="text-[10px] uppercase tracking-widest text-slate-500 mb-1 block">"Author"</label>
                <input
                    type="text"
                    class="w-full px-3 py-2 rounded-md bg-slate-800 border border-slate-700 text-slate-100 text-sm focus:border-teal-400 focus:outline-none"
                    placeholder="GitHub username"
                    prop:value=move || author.get()
                    on:input=move |ev| author.set(event_target_value(&ev))
                />
            </div>

            <div>
                <label class="text-[10px] uppercase tracking-widest text-slate-500 mb-1 block">"Tags"</label>
                <div class="flex flex-wrap gap-1 mb-1">
                    {move || tags.get().into_iter().map(|t| {
                        let t_remove = t.clone();
                        view! {
                            <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] bg-teal-400/20 text-teal-200 border border-teal-400/40">
                                {"#"}{t}
                                <button
                                    class="hover:text-red-300"
                                    on:click=move |_| {
                                        let t = t_remove.clone();
                                        tags.update(|v| v.retain(|x| x != &t));
                                    }
                                >"×"</button>
                            </span>
                        }
                    }).collect_view()}
                </div>
                <input
                    type="text"
                    class="w-full px-3 py-2 rounded-md bg-slate-800 border border-slate-700 text-slate-100 text-sm focus:border-teal-400 focus:outline-none"
                    placeholder="type a tag and press Enter"
                    prop:value=move || tag_input.get()
                    on:input=move |ev| tag_input.set(event_target_value(&ev))
                    on:keydown=move |ev| {
                        let k = ev.key();
                        if k == "Enter" || k == "," || k == " " {
                            ev.prevent_default();
                            add_tag(tag_input.get_untracked());
                        }
                    }
                    on:blur=move |_| add_tag(tag_input.get_untracked())
                />
                <div class="flex flex-wrap gap-1 mt-1">
                    {move || tag_suggestions.get().into_iter().map(|t| {
                        let t_click = t.clone();
                        view! {
                            <button
                                class="px-2 py-0.5 rounded text-[10px] bg-slate-800 text-slate-400 border border-slate-700 hover:text-teal-200 hover:border-teal-400/40 transition-colors"
                                on:click=move |_| add_tag(t_click.clone())
                            >
                                {"+ #"}{t}
                            </button>
                        }
                    }).collect_view()}
                </div>
            </div>

            <div>
                <div class="flex items-center justify-between mb-1">
                    <label class="text-[10px] uppercase tracking-widest text-slate-500">
                        {move || match node_type.get() {
                            NodeType::Concept => "Summary",
                            NodeType::Decision => "Context",
                            NodeType::Meeting => "Summary / Notes",
                            NodeType::Tag => "Body",
                        }}
                    </label>
                    <button
                        class="text-[10px] uppercase tracking-widest text-slate-400 hover:text-teal-300 transition-colors"
                        on:click=move |_| show_preview.update(|v| *v = !*v)
                    >
                        {move || if show_preview.get() { "Edit" } else { "Preview" }}
                    </button>
                </div>
                <Show
                    when=move || show_preview.get()
                    fallback=move || view! {
                        <textarea
                            class="w-full px-3 py-2 rounded-md bg-slate-800 border border-slate-700 text-slate-100 text-sm focus:border-teal-400 focus:outline-none min-h-[180px] resize-y font-mono"
                            placeholder="Write the main content here (Markdown supported)…"
                            prop:value=move || body.get()
                            on:input=move |ev| body.set(event_target_value(&ev))
                        />
                    }
                >
                    <div class="px-3 py-2 rounded-md bg-slate-950 border border-slate-800 min-h-[180px]">
                        {move || {
                            let b = body.get();
                            if b.trim().is_empty() {
                                view! {
                                    <div class="text-slate-600 text-xs italic">"Nothing to preview yet."</div>
                                }.into_any()
                            } else {
                                view! {
                                    <article
                                        class="prose prose-invert prose-sm max-w-none"
                                        inner_html=preview_html.get()
                                    ></article>
                                }.into_any()
                            }
                        }}
                    </div>
                </Show>
            </div>

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
                                    class="w-full text-left px-2 py-1 rounded text-xs hover:bg-slate-700 transition-colors"
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
                            <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] bg-teal-400/20 text-teal-200 border border-teal-400/40">
                                {display}
                                <button
                                    class="hover:text-red-300"
                                    on:click=move |_| {
                                        let p = path_for_remove.clone();
                                        selected_related.update(|r| r.retain(|x| x != &p));
                                    }
                                >"×"</button>
                            </span>
                        }
                    }).collect_view()}
                </div>
            </div>

            <div class="pt-2 border-t border-slate-800">
                <button
                    class="w-full px-4 py-2 rounded-md bg-teal-500 text-slate-950 text-sm font-semibold hover:bg-teal-400 transition-colors disabled:opacity-50"
                    disabled=move || saving.get() || title.with(|t| t.is_empty())
                    on:click=on_submit
                >
                    {move || if saving.get() { "Saving…" } else { "Create & Commit" }}
                </button>
                <p class="text-[11px] text-slate-400 mt-2 text-center">
                    {move || status_msg.get()}
                </p>
                <p class="text-[10px] text-slate-600 mt-1 text-center">
                    "Frontmatter is auto-generated from the Brain templates."
                </p>
            </div>
        </aside>
    }
}
