use leptos::prelude::*;
use std::collections::BTreeMap;

use super::components::RemovableBadge;
use super::draft::{self, Draft};
use super::types::{EditMode, NodeType};
use crate::api::{AppConfig, get_current_user, load_brain_template};

/// Smart editor form that enforces Brain templates programmatically.
#[component]
pub fn EditorPanel(
    /// Available nodes for the "Related / See also" forced-linking picker.
    node_titles: Vec<(String, String)>,
    /// Existing tag vocabulary across the repo.
    all_tags: Vec<String>,
    /// Current editor mode — lets us detect create vs. edit and access the prefill.
    edit_mode: RwSignal<EditMode>,
    /// Bump to trigger a graph refetch after save (replaces full-page reload).
    graph_version: RwSignal<u64>,
) -> impl IntoView {
    let node_type = RwSignal::new(NodeType::Concept);
    let title = RwSignal::new(String::new());
    let author = RwSignal::new(String::new());
    let tags = RwSignal::new(Vec::<String>::new());
    let body = RwSignal::new(String::new());
    let selected_related = RwSignal::new(Vec::<String>::new());
    let folder = RwSignal::new(String::new());
    let all_folders = Resource::new(
        || (),
        |_| async { crate::api::list_brain_folders().await.unwrap_or_default() },
    );
    let status_msg = RwSignal::new(String::new());
    let saving = RwSignal::new(false);
    let edit_path = RwSignal::new(Option::<String>::None);
    let edit_sha = RwSignal::new(Option::<String>::None);
    let preserved_frontmatter = RwSignal::new(Option::<BTreeMap<String, serde_yaml::Value>>::None);
    let frontmatter_malformed = RwSignal::new(false);
    let custom_msg_open = RwSignal::new(read_custom_msg_pref());
    let custom_msg = RwSignal::new(String::new());
    Effect::new(move |_| {
        write_custom_msg_pref(custom_msg_open.get());
    });

    // Prefill from EditMode::Edit(prefill). Runs once per transition into Edit mode.
    let prefilled_for = RwSignal::new(Option::<String>::None);
    Effect::new(move |_| {
        if let EditMode::Edit(p) = edit_mode.get() {
            if prefilled_for.get_untracked().as_deref() == Some(&p.path) {
                return;
            }
            prefilled_for.set(Some(p.path.clone()));
            if let Some(nt) = p.node_type {
                node_type.set(nt);
            }
            title.set(p.title);
            if !p.author.is_empty() {
                author.set(p.author);
            }
            tags.set(p.tags);
            body.set(p.body);
            selected_related.set(p.related);
            edit_path.set(Some(p.path));
            edit_sha.set(Some(p.sha));
            preserved_frontmatter.set(if p.frontmatter.is_empty() {
                None
            } else {
                Some(p.frontmatter)
            });
            frontmatter_malformed.set(p.frontmatter_malformed);
            folder.set(String::new());
        } else {
            prefilled_for.set(None);
            if matches!(edit_mode.get(), EditMode::New) {
                edit_path.set(None);
                edit_sha.set(None);
                preserved_frontmatter.set(None);
                frontmatter_malformed.set(false);
                folder.set(String::new());
            }
        }
    });

    let is_edit = Memo::new(move |_| edit_sha.with(|s| s.is_some()));

    // Fetch the Brain template and prefill the body textarea when in New mode.
    let template_applied_for = RwSignal::new(Option::<NodeType>::None);
    Effect::new(move |_| {
        if is_edit.get() {
            return;
        }
        if !matches!(edit_mode.get(), EditMode::New) {
            template_applied_for.set(None);
            return;
        }
        let nt = node_type.get();
        if template_applied_for.get_untracked() == Some(nt) {
            return;
        }
        let current_body = body.get_untracked();
        let last_applied = template_applied_for.get_untracked();
        let safe_to_replace = current_body.trim().is_empty() || last_applied.is_some();
        if !safe_to_replace {
            return;
        }
        template_applied_for.set(Some(nt));
        #[cfg(not(feature = "ssr"))]
        {
            leptos::task::spawn_local(async move {
                match load_brain_template(nt).await {
                    Ok(t) if !t.is_empty() => body.set(t),
                    Ok(_) => {}
                    Err(_) => {}
                }
            });
        }
        #[cfg(feature = "ssr")]
        {
            let _ = load_brain_template;
        }
    });

    // Auto-fill author from the current GitHub session (once) in New mode.
    let session_user = Resource::new(|| (), |_| async { get_current_user().await });
    Effect::new(move |_| {
        if is_edit.get() {
            return;
        }
        if let Some(Ok(Some(login))) = session_user.get()
            && author.with_untracked(|a| a.is_empty())
        {
            author.set(login);
        }
    });

    // --- Auto-save drafts to localStorage -----------------------------------
    // Key drafts by `<org>/<repo>:<path|new>` so drafts from a different
    // deployment target don't collide and each edited file keeps its own draft.
    let app_config = use_context::<Resource<Result<AppConfig, ServerFnError>>>();
    let repo_scope = Memo::new(move |_| {
        app_config
            .and_then(|r| r.get())
            .and_then(|r| r.ok())
            .map(|c| format!("{}/{}", c.target.org, c.target.repo))
            .unwrap_or_default()
    });
    let draft_key = Memo::new(move |_| {
        let scope = repo_scope.get();
        if scope.is_empty() {
            return None;
        }
        Some(draft::storage_key(&scope, edit_path.get().as_deref()))
    });

    let restore_banner = RwSignal::new(Option::<Draft>::None);

    // Offer to restore once per editor session, gated on the draft_key being
    // ready (i.e. config has loaded). Edit-mode drafts are discarded silently
    // if the base_sha no longer matches — we don't want to revert someone
    // else's commit when the user clicks Restore.
    let restore_checked = RwSignal::new(false);
    Effect::new(move |_| {
        if restore_checked.get() {
            return;
        }
        let Some(key) = draft_key.get() else {
            return;
        };
        let Some(loaded) = draft::load(&key) else {
            restore_checked.set(true);
            return;
        };
        let current_sha = edit_sha.get_untracked();
        let stale = match (&loaded.base_sha, &current_sha) {
            (Some(draft_sha), Some(live_sha)) => draft_sha != live_sha,
            (Some(_), None) => true, // draft is for an edit, but we're in New mode
            _ => false,
        };
        if stale {
            draft::clear(&key);
        } else {
            restore_banner.set(Some(loaded));
        }
        restore_checked.set(true);
    });

    // Debounced write: 2s after the user stops typing, persist the form state.
    // `Timeout` isn't Send/Sync — use the local-storage variant so dropping
    // the previous handle cancels the pending timer.
    #[cfg(feature = "hydrate")]
    let debounce_handle: StoredValue<
        Option<gloo_timers::callback::Timeout>,
        leptos::prelude::LocalStorage,
    > = StoredValue::new_local(None);
    Effect::new(move |_| {
        // Subscribe to everything the user can edit.
        let nt = node_type.get();
        let t = title.get();
        let a = author.get();
        let tg = tags.get();
        let b = body.get();
        let r = selected_related.get();
        let f = folder.get();
        let Some(key) = draft_key.get() else {
            return;
        };
        // Don't persist an empty, unmodified form — avoids writing a blank
        // draft on every mount just from default signal reads.
        if t.is_empty() && b.is_empty() && tg.is_empty() && r.is_empty() && f.is_empty() {
            return;
        }
        let base_sha = edit_sha.get_untracked();
        let preserved = preserved_frontmatter.get_untracked();

        #[cfg(feature = "hydrate")]
        {
            let draft = Draft {
                node_type: nt,
                title: t,
                author: a,
                tags: tg,
                body: b,
                related: r,
                folder: Some(f),
                saved_at: draft::now_secs(),
                base_sha,
                preserved_frontmatter: preserved,
                frontmatter_malformed: frontmatter_malformed.get_untracked(),
            };
            let key_for_timeout = key.clone();
            let new_handle = gloo_timers::callback::Timeout::new(2_000, move || {
                draft::save(&key_for_timeout, &draft);
            });
            debounce_handle.set_value(Some(new_handle));
        }
        #[cfg(not(feature = "hydrate"))]
        {
            let _ = (nt, a, base_sha, key, preserved);
        }
    });

    let restore_draft = move || {
        let Some(d) = restore_banner.get_untracked() else {
            return;
        };
        node_type.set(d.node_type);
        title.set(d.title);
        if !d.author.is_empty() {
            author.set(d.author);
        }
        tags.set(d.tags);
        body.set(d.body);
        selected_related.set(d.related);
        if let Some(f) = d.folder {
            folder.set(f);
        }
        preserved_frontmatter.set(d.preserved_frontmatter);
        frontmatter_malformed.set(d.frontmatter_malformed);
        restore_banner.set(None);
    };
    let discard_draft = move || {
        if let Some(key) = draft_key.get_untracked() {
            draft::clear(&key);
        }
        restore_banner.set(None);
    };

    let node_titles_stored = StoredValue::new(node_titles);
    let all_tags_stored = StoredValue::new(all_tags);

    let on_submit = move |_| {
        let updating = is_edit.get_untracked();
        let _payload = crate::knowledge::types::BrainFilePayload {
            node_type: node_type.get_untracked(),
            title: title.get_untracked(),
            author: author.get_untracked(),
            tags: tags.get_untracked(),
            body: body.get_untracked(),
            related: selected_related.get_untracked(),
            folder: Some(folder.get_untracked()),
            path: edit_path.get_untracked(),
            sha: edit_sha.get_untracked(),
            commit_message: if custom_msg_open.get_untracked() {
                let m = custom_msg.get_untracked();
                let t = m.trim();
                if t.is_empty() {
                    None
                } else {
                    Some(t.to_string())
                }
            } else {
                None
            },
            preserved_frontmatter: preserved_frontmatter.get_untracked(),
            frontmatter_malformed: frontmatter_malformed.get_untracked(),
        };

        saving.set(true);
        status_msg.set(if updating {
            "Updating…".to_string()
        } else {
            "Saving…".to_string()
        });

        #[cfg(not(feature = "ssr"))]
        {
            use crate::api::save_brain_file;
            let draft_key_snapshot = draft_key.get_untracked();
            leptos::task::spawn_local(async move {
                match save_brain_file(_payload).await {
                    Ok(path) => {
                        status_msg.set(if updating {
                            format!("Updated: {path}")
                        } else {
                            format!("Created: {path}")
                        });
                        saving.set(false);
                        if let Some(key) = draft_key_snapshot {
                            draft::clear(&key);
                        }
                        graph_version.update(|v| *v += 1);
                    }
                    Err(e) => {
                        status_msg.set(format!("Error: {e}"));
                        saving.set(false);
                    }
                }
            });
        }
        #[cfg(feature = "ssr")]
        {
            let _ = &graph_version;
        }
    };

    // --- Type/folder mismatch banner ---------------------------------------
    // When editing, if the file lives in the canonical directory of a *different*
    // type than the one currently selected, offer to move it. We only trigger
    // when the current dir maps cleanly to a known type — custom paths (e.g.
    // `drafts/q3/foo.md`) are treated as intentional and left alone.
    let mismatch = Memo::new(move |_| {
        if !is_edit.get() {
            return None;
        }
        let path = edit_path.get()?;
        let (dir, _file) = path.rsplit_once('/')?;
        let current_type = NodeType::from_directory(dir)?;
        let target = node_type.get();
        if current_type == target || target.directory().is_empty() {
            return None;
        }
        Some((path, current_type, target))
    });

    let moving = RwSignal::new(false);
    let move_error = RwSignal::new(String::new());

    let do_move = move || {
        let Some((old_path, _from, to)) = mismatch.get_untracked() else {
            return;
        };
        let Some(sha) = edit_sha.get_untracked() else {
            return;
        };
        let filename = old_path.rsplit('/').next().unwrap_or(&old_path).to_string();
        let new_path = format!("{}/{}", to.directory(), filename);

        moving.set(true);
        move_error.set(String::new());
        #[cfg(not(feature = "ssr"))]
        {
            use crate::api::rename_brain_file;
            leptos::task::spawn_local(async move {
                match rename_brain_file(old_path, new_path.clone(), sha, None).await {
                    Ok(res) => {
                        edit_path.set(Some(res.new_path.clone()));
                        // sha is stale after the move (create + delete commits);
                        // exit edit mode so the user reopens with a fresh sha.
                        edit_mode.set(EditMode::Closed);
                        moving.set(false);
                        graph_version.update(|v| *v += 1);
                    }
                    Err(e) => {
                        move_error.set(format!("Move failed: {e}"));
                        moving.set(false);
                    }
                }
            });
        }
        #[cfg(feature = "ssr")]
        {
            let _ = (new_path, sha);
        }
    };

    view! {
        <aside class="w-[420px] shrink-0 border-r border-slate-800 bg-slate-900/60 p-5 space-y-4 overflow-y-auto">
            <div class="flex items-center justify-between mb-2">
                <h2 class="text-xs font-semibold tracking-widest uppercase text-teal-400">
                    {move || if is_edit.get() { "Edit Document" } else { "New Document" }}
                </h2>
                <button
                    class="text-slate-500 hover:text-slate-200 text-xs transition-colors focus:outline-none focus:ring-1 focus:ring-slate-500 rounded px-1"
                    on:click=move |_| edit_mode.set(EditMode::Closed)
                >
                    "Cancel"
                </button>
            </div>
            <Show when=move || is_edit.get()>
                <div class="text-[10px] text-slate-500 -mt-1 mb-1">
                    {move || edit_path.get().unwrap_or_default()}
                </div>
            </Show>

            <Show when=move || restore_banner.with(|b| b.is_some())>
                {
                    let restore = restore_draft;
                    let discard = discard_draft;
                    view! {
                        <div class="px-3 py-2 rounded-md bg-amber-500/10 border border-amber-400/40 text-amber-100 text-xs space-y-2">
                            <div>
                                {move || {
                                    let when = restore_banner
                                        .with(|b| b.as_ref().map(|d| d.saved_at).unwrap_or(0));
                                    format!(
                                        "Unsaved draft found — saved {}.",
                                        draft::relative_time(when, draft::now_secs())
                                    )
                                }}
                            </div>
                            <div class="flex gap-2">
                                <button
                                    class="px-3 py-1 rounded bg-amber-400/30 border border-amber-300/50 text-amber-50 hover:bg-amber-400/50 transition-colors focus:outline-none focus:ring-1 focus:ring-amber-500"
                                    on:click=move |_| restore()
                                >
                                    "Restore"
                                </button>
                                <button
                                    class="px-3 py-1 rounded bg-slate-800 border border-slate-700 text-slate-300 hover:text-slate-100 transition-colors focus:outline-none focus:ring-1 focus:ring-slate-500"
                                    on:click=move |_| discard()
                                >
                                    "Discard"
                                </button>
                            </div>
                        </div>
                    }
                }
            </Show>

            <FrontmatterFields node_type=node_type title=title author=author />

            <Show when=move || mismatch.with(|m| m.is_some())>
                {
                    let do_move = do_move;
                    view! {
                        <div class="px-3 py-2 rounded-md bg-amber-500/10 border border-amber-400/40 text-amber-100 text-xs space-y-2">
                            <div>
                                {move || mismatch.with(|m| m.as_ref().map(|(path, from, to)| {
                                    let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                                    format!(
                                        "This {} lives in `{}/` (the {} folder). Move to `{}/`?",
                                        to.label(), dir, from.label(), to.directory(),
                                    )
                                }).unwrap_or_default())}
                            </div>
                            <div class="flex gap-2 items-center">
                                <button
                                    class="px-3 py-1 rounded bg-amber-400/30 border border-amber-300/50 text-amber-50 hover:bg-amber-400/50 transition-colors focus:outline-none focus:ring-1 focus:ring-amber-500 disabled:opacity-50"
                                    disabled=move || moving.get()
                                    on:click=move |_| do_move()
                                >
                                    {move || if moving.get() { "Moving…" } else { "Move file" }}
                                </button>
                                <Show when=move || !move_error.with(String::is_empty)>
                                    <span class="text-rose-300">{move || move_error.get()}</span>
                                </Show>
                            </div>
                        </div>
                    }
                }
            </Show>

            <LocationPicker folder=folder node_type=node_type all_folders=all_folders is_edit=is_edit />
            <TagInput tags=tags all_tags=all_tags_stored />
            <MarkdownPreview node_type=node_type.into() body=body />
            <RelatedLinksPicker selected_related=selected_related node_titles=node_titles_stored />

            <div class="pt-2 border-t border-slate-800 space-y-2">
                <div>
                    <button
                        class="text-[10px] uppercase tracking-widest text-slate-400 hover:text-teal-300 transition-colors focus:outline-none focus:ring-1 focus:ring-slate-500 rounded px-1"
                        on:click=move |_| custom_msg_open.update(|v| *v = !*v)
                    >
                        {move || if custom_msg_open.get() { "▾ Custom commit message" } else { "▸ Custom commit message" }}
                    </button>
                    <Show when=move || custom_msg_open.get()>
                        <input
                            type="text"
                            maxlength="200"
                            class="mt-1 w-full px-3 py-2 rounded-md bg-slate-800 border border-slate-700 text-slate-100 text-xs focus:border-teal-400 focus:outline-none font-mono"
                            placeholder=move || {
                                let updating = is_edit.get();
                                let path = edit_path.get().unwrap_or_else(|| "…".to_string());
                                if updating {
                                    format!("Update {path} via Brain UI")
                                } else {
                                    format!("Create {path} via Brain UI")
                                }
                            }
                            prop:value=move || custom_msg.get()
                            on:input=move |ev| custom_msg.set(event_target_value(&ev))
                        />
                        <p class="text-[10px] text-slate-600 mt-1">
                            "Leave blank to use the auto-generated message."
                        </p>
                    </Show>
                </div>
                <button
                    class="w-full px-4 py-2 rounded-md bg-teal-500 text-slate-950 text-sm font-semibold hover:bg-teal-400 transition-colors focus:outline-none focus:ring-2 focus:ring-teal-500 focus:ring-offset-2 focus:ring-offset-slate-900 disabled:opacity-50"
                    disabled=move || saving.get() || title.with(|t| t.is_empty())
                    on:click=on_submit
                >
                    {move || match (saving.get(), is_edit.get()) {
                        (true, true) => "Updating…",
                        (true, false) => "Saving…",
                        (false, true) => "Update & Commit",
                        (false, false) => "Create & Commit",
                    }}
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

// ---------------------------------------------------------------------------
// Sub-components
// ---------------------------------------------------------------------------

/// Type selector + title + author fields.
#[component]
fn FrontmatterFields(
    node_type: RwSignal<NodeType>,
    title: RwSignal<String>,
    author: RwSignal<String>,
) -> impl IntoView {
    view! {
        <div>
            <label class="text-[10px] uppercase tracking-widest text-slate-500 mb-1 block">"Type"</label>
            <div class="flex flex-wrap gap-2">
                {NodeType::CREATABLE.iter().map(|t| {
                    let t = *t;
                    let is_active = Memo::new(move |_| node_type.get() == t);
                    view! {
                        <button
                            class="px-3 py-1 rounded-full text-xs border transition-colors flex items-center gap-2 focus:outline-none focus:ring-1 focus:ring-slate-500"
                            class=("bg-slate-100", move || is_active.get())
                            class=("text-slate-900", move || is_active.get())
                            class=("border-slate-100", move || is_active.get())
                            class=("text-slate-300", move || !is_active.get())
                            class=("border-slate-700", move || !is_active.get())
                            on:click=move |_| node_type.set(t)
                        >
                            <span class="inline-block w-2 h-2 rounded-full" style=format!("background:{}", t.accent_var())></span>
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
    }
}

/// Tag pills, autocomplete input, and suggestion buttons.
#[component]
fn TagInput(tags: RwSignal<Vec<String>>, all_tags: StoredValue<Vec<String>>) -> impl IntoView {
    let tag_input = RwSignal::new(String::new());

    let tag_suggestions = Memo::new(move |_| {
        let query = tag_input.get().to_lowercase();
        if query.is_empty() {
            return vec![];
        }
        let current = tags.get();
        all_tags.with_value(|all| {
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

    view! {
        <div>
            <label class="text-[10px] uppercase tracking-widest text-slate-500 mb-1 block">"Tags"</label>
            <div class="flex flex-wrap gap-1 mb-1">
                {move || tags.get().into_iter().map(|t| {
                    let t_remove = t.clone();
                    view! {
                        <RemovableBadge
                            label=t
                            prefix="#"
                            on_remove=move || {
                                let t = t_remove.clone();
                                tags.update(|v| v.retain(|x| x != &t));
                            }
                        />
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
                            class="px-2 py-0.5 rounded text-[10px] bg-slate-800 text-slate-400 border border-slate-700 hover:text-teal-200 hover:border-teal-400/40 transition-colors focus:outline-none focus:ring-1 focus:ring-slate-500"
                            on:click=move |_| add_tag(t_click.clone())
                        >
                            {"+ #"}{t}
                        </button>
                    }
                }).collect_view()}
            </div>
        </div>
    }
}

/// Edit / preview toggle for the markdown body.
#[component]
fn MarkdownPreview(node_type: Signal<NodeType>, body: RwSignal<String>) -> impl IntoView {
    let show_preview = RwSignal::new(false);
    let preview_html = Memo::new(move |_| crate::markdown::render(&body.get()));
    let upload_status = RwSignal::new(String::new());
    let dragging = RwSignal::new(false);

    view! {
        <div>
            <div class="flex items-center justify-between mb-1">
                <label class="text-[10px] uppercase tracking-widest text-slate-500">
                    {move || match node_type.get() {
                        NodeType::Concept => "Summary",
                        NodeType::Decision => "Context",
                        NodeType::Meeting => "Summary / Notes",
                        NodeType::PostMortem => "Incident Summary",
                        NodeType::Preventivo => "Riepilogo",
                        NodeType::Runbook => "Description",
                        NodeType::Tag => "Body",
                    }}
                </label>
                <button
                    class="text-[10px] uppercase tracking-widest text-slate-400 hover:text-teal-300 transition-colors focus:outline-none focus:ring-1 focus:ring-slate-500 rounded px-1"
                    on:click=move |_| show_preview.update(|v| *v = !*v)
                >
                    {move || if show_preview.get() { "Edit" } else { "Preview" }}
                </button>
            </div>
            <Show
                when=move || show_preview.get()
                fallback=move || view! {
                    <textarea
                        class="w-full px-3 py-2 rounded-md bg-slate-800 border text-slate-100 text-sm focus:border-teal-400 focus:outline-none min-h-[180px] resize-y font-mono transition-colors"
                        class=("bg-slate-800", move || !dragging.get())
                        class=("border-slate-700", move || !dragging.get())
                        class=("bg-teal-500/10", move || dragging.get())
                        class=("border-teal-400", move || dragging.get())
                        placeholder="Write the main content here (Markdown supported). Drop images to upload."
                        prop:value=move || body.get()
                        on:input=move |ev| body.set(event_target_value(&ev))
                        on:dragover=move |ev| {
                            ev.prevent_default();
                            dragging.set(true);
                        }
                        on:dragleave=move |_| dragging.set(false)
                        on:drop=move |ev| {
                            ev.prevent_default();
                            dragging.set(false);
                            #[cfg(feature = "hydrate")]
                            handle_image_drop(ev, body, upload_status);
                            #[cfg(not(feature = "hydrate"))]
                            { let _ = (upload_status, &body); }
                        }
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
                                    class="prose prose-invert max-w-prose"
                                    inner_html=preview_html.get()
                                ></article>
                            }.into_any()
                        }
                    }}
                </div>
            </Show>
            <Show when=move || !upload_status.get().is_empty()>
                <p class="text-[10px] text-teal-300 mt-1">{move || upload_status.get()}</p>
            </Show>
        </div>
    }
}

/// Upload every image file from a drop event, inserting a markdown image tag
/// into the body for each one as it completes. Non-image files are skipped
/// silently; per-file errors surface in `status` but don't abort siblings.
#[cfg(feature = "hydrate")]
fn handle_image_drop(ev: leptos::ev::DragEvent, body: RwSignal<String>, status: RwSignal<String>) {
    use crate::api::upload_asset;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    let Some(dt) = ev.data_transfer() else {
        return;
    };
    let Some(files) = dt.files() else {
        return;
    };
    let count = files.length();
    if count == 0 {
        return;
    }
    for i in 0..count {
        let Some(file) = files.get(i) else { continue };
        let mime = file.type_();
        if !mime.starts_with("image/") {
            continue;
        }
        let filename = file.name();
        let file_for_task = file.clone();
        status.set(format!("Uploading {filename}…"));
        leptos::task::spawn_local(async move {
            let buf_promise = file_for_task.array_buffer();
            let buf = match JsFuture::from(buf_promise).await {
                Ok(v) => v,
                Err(_) => {
                    status.set(format!("Read failed: {filename}"));
                    return;
                }
            };
            let Ok(array) = buf.dyn_into::<js_sys::ArrayBuffer>() else {
                status.set(format!("Read failed: {filename}"));
                return;
            };
            let bytes = js_sys::Uint8Array::new(&array).to_vec();
            let alt = strip_ext(&filename);
            match upload_asset(filename.clone(), bytes).await {
                Ok(path) => {
                    let snippet = format!("\n\n![{alt}](/{path})\n");
                    body.update(|b| b.push_str(&snippet));
                    status.set(format!("Uploaded {path}"));
                }
                Err(e) => {
                    status.set(format!("Upload failed ({filename}): {e}"));
                }
            }
        });
    }
}

#[cfg(feature = "hydrate")]
fn strip_ext(filename: &str) -> String {
    match filename.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
        _ => filename.to_string(),
    }
}

#[cfg(not(feature = "ssr"))]
const CUSTOM_MSG_PREF_KEY: &str = "brain-ui:commit-msg-open";

#[cfg(not(feature = "ssr"))]
fn read_custom_msg_pref() -> bool {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(CUSTOM_MSG_PREF_KEY).ok().flatten())
        .is_some_and(|v| v == "1")
}

#[cfg(feature = "ssr")]
fn read_custom_msg_pref() -> bool {
    false
}

#[cfg(not(feature = "ssr"))]
fn write_custom_msg_pref(open: bool) {
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.set_item(CUSTOM_MSG_PREF_KEY, if open { "1" } else { "0" });
    }
}

#[cfg(feature = "ssr")]
fn write_custom_msg_pref(_open: bool) {}

/// Searchable node picker for forced "Related / See also" linking.
#[component]
fn RelatedLinksPicker(
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

#[component]
fn LocationPicker(
    folder: RwSignal<String>,
    node_type: RwSignal<NodeType>,
    all_folders: Resource<Vec<String>>,
    is_edit: Memo<bool>,
) -> impl IntoView {
    view! {
        <Show when=move || !is_edit.get()>
            <div>
                <label class="text-[10px] uppercase tracking-widest text-slate-500 mb-1 block">"Location"</label>
                <div class="relative">
                    <input
                        type="text"
                        list="brain-folders"
                        class="w-full px-3 py-2 rounded-md bg-slate-800 border border-slate-700 text-slate-100 text-sm focus:border-teal-400 focus:outline-none"
                        placeholder=move || node_type.get().directory()
                        prop:value=move || folder.get()
                        on:input=move |ev| folder.set(event_target_value(&ev))
                    />
                    <datalist id="brain-folders">
                        <Suspense fallback=|| ()>
                            {move || all_folders.get().unwrap_or_default().into_iter().map(|f| {
                                view! { <option value=f /> }
                            }).collect_view()}
                        </Suspense>
                    </datalist>
                </div>
                <p class="text-[10px] text-slate-500 mt-1 leading-relaxed">
                    "Leave blank for default. Create new folders implicitly by typing a path like 'drafts/q3'."
                </p>
            </div>
        </Show>
    }
}
