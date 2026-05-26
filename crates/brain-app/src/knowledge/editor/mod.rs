use leptos::prelude::*;
use std::collections::BTreeMap;

use super::draft::{self, Draft};
use super::types::EditMode;
#[cfg(not(feature = "ssr"))]
use crate::api::WriteMode;
use crate::api::{AppConfig, get_current_user, load_brain_template};
use brain_domain::TargetRef;

mod frontmatter;
mod location;
mod markdown;
mod related;
mod tags;

use frontmatter::{
    ExtraFrontmatterFields, FrontmatterFields, WorkItemFields, frontmatter_string_fields,
};
use location::{LocationPicker, slugify_title};
use markdown::MarkdownPreview;
use related::RelatedLinksPicker;
use tags::TagInput;

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
    config: brain_domain::BrainConfig,
) -> impl IntoView {
    let active_target = StoredValue::new(expect_context::<TargetRef>());
    let config = StoredValue::new(config);
    let node_type = RwSignal::new(config.with_value(|c| c.default_spec().name.clone()));
    let title = RwSignal::new(String::new());
    let author = RwSignal::new(String::new());
    let tags = RwSignal::new(Vec::<String>::new());
    let body = RwSignal::new(String::new());
    let selected_related = RwSignal::new(Vec::<String>::new());
    let wi_state = RwSignal::new(String::new());
    let wi_system_of_record = RwSignal::new(String::from("brain"));
    let wi_assignees = RwSignal::new(String::new());
    let folder = RwSignal::new(String::new());
    let all_folders = Resource::new(
        || (),
        move |_| {
            let target = active_target.get_value();
            async move {
                crate::api::list_brain_folders(target)
                    .await
                    .unwrap_or_default()
            }
        },
    );
    let status_msg = RwSignal::new(String::new());
    let saving = RwSignal::new(false);
    let edit_path = RwSignal::new(Option::<String>::None);
    let edit_sha = RwSignal::new(Option::<String>::None);
    let preserved_frontmatter = RwSignal::new(Option::<BTreeMap<String, serde_yaml::Value>>::None);
    let extra_frontmatter = RwSignal::new(BTreeMap::<String, String>::new());
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
            let effective_node_type = p
                .node_type
                .clone()
                .unwrap_or_else(|| node_type.get_untracked());
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
            wi_state.set(work_item_status_from_frontmatter(&p.frontmatter));
            wi_system_of_record.set(
                p.frontmatter
                    .get("system_of_record")
                    .and_then(|v| v.as_str())
                    .unwrap_or("brain")
                    .to_string(),
            );
            wi_assignees.set(
                p.frontmatter
                    .get("assignees")
                    .and_then(|v| v.as_sequence())
                    .map(|seq| {
                        seq.iter()
                            .filter_map(|v| v.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default(),
            );
            edit_path.set(Some(p.path));
            edit_sha.set(Some(p.sha));
            preserved_frontmatter.set(if p.frontmatter.is_empty() {
                None
            } else {
                Some(p.frontmatter)
            });
            extra_frontmatter.set(config.with_value(|c| {
                frontmatter_string_fields(
                    c,
                    &effective_node_type,
                    preserved_frontmatter.get_untracked().as_ref(),
                )
            }));
            frontmatter_malformed.set(p.frontmatter_malformed);
            folder.set(String::new());
        } else {
            prefilled_for.set(None);
            if matches!(edit_mode.get(), EditMode::New) {
                edit_path.set(None);
                edit_sha.set(None);
                preserved_frontmatter.set(None);
                extra_frontmatter.set(config.with_value(|c| {
                    frontmatter_string_fields(c, &node_type.get_untracked(), None)
                }));
                frontmatter_malformed.set(false);
                folder.set(String::new());
                wi_state.set(String::from("todo"));
                wi_system_of_record.set(String::from("brain"));
                wi_assignees.set(String::new());
            }
        }
    });

    let is_edit = Memo::new(move |_| edit_sha.with(|s| s.is_some()));

    let seeded_frontmatter_for = RwSignal::new(Option::<String>::None);
    Effect::new(move |_| {
        if is_edit.get() || !matches!(edit_mode.get(), EditMode::New) {
            seeded_frontmatter_for.set(None);
            return;
        }
        let current_type = node_type.get();
        if seeded_frontmatter_for.get_untracked().as_deref() == Some(&current_type) {
            return;
        }
        extra_frontmatter
            .set(config.with_value(|c| frontmatter_string_fields(c, &current_type, None)));
        seeded_frontmatter_for.set(Some(current_type));
    });

    // Fetch the Brain template and prefill the body textarea when in New mode.
    let template_applied_for = RwSignal::new(Option::<String>::None);
    Effect::new(move |_| {
        if is_edit.get() {
            return;
        }
        if !matches!(edit_mode.get(), EditMode::New) {
            template_applied_for.set(None);
            return;
        }
        let nt = node_type.get();
        if template_applied_for.get_untracked() == Some(nt.clone()) {
            return;
        }
        let current_body = body.get_untracked();
        let last_applied = template_applied_for.get_untracked();
        let safe_to_replace = current_body.trim().is_empty() || last_applied.is_some();
        if !safe_to_replace {
            return;
        }
        template_applied_for.set(Some(nt.clone()));
        #[cfg(not(feature = "ssr"))]
        {
            leptos::task::spawn_local(async move {
                let target = active_target.get_value();
                match load_brain_template(target, nt).await {
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
    let app_config = use_context::<Resource<Result<AppConfig, crate::api::ApiError>>>();
    let app_config_for_scope = app_config;
    let repo_scope = Memo::new(move |_| {
        app_config_for_scope
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
        let extra = extra_frontmatter.get_untracked();

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
                extra_frontmatter: extra,
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
            let _ = (nt, a, base_sha, key, preserved, extra);
        }
    });

    let restore_draft = move || {
        let Some(d) = restore_banner.get_untracked() else {
            return;
        };
        node_type.set(d.node_type.clone());
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
        extra_frontmatter.set(d.extra_frontmatter);
        seeded_frontmatter_for.set(Some(d.node_type));
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
    let markdown_file_path = Memo::new(move |_| {
        if let Some(path) = edit_path.get() {
            return Some(path);
        }

        let slug = slugify_title(&title.get());
        if slug.is_empty() {
            return None;
        }

        let selected_folder = folder.get();
        let dir = selected_folder.trim().trim_matches('/').to_string();
        let dir = if dir.is_empty() {
            let nt = node_type.get();
            config.with_value(|c| {
                c.lookup(&nt)
                    .map(|s| s.directory.trim_matches('/').to_string())
                    .unwrap_or_default()
            })
        } else {
            dir
        };

        Some(if dir.is_empty() {
            format!("{slug}.md")
        } else {
            format!("{dir}/{slug}.md")
        })
    });

    let on_submit = move |_| {
        let updating = is_edit.get_untracked();
        let nt = node_type.get_untracked();
        let is_work_item =
            config.with_value(|c| c.lookup(&nt).map(|s| s.is_work_item()).unwrap_or(false));

        // For work item types, bake the form-controlled operational fields
        // into preserved_frontmatter so merge_frontmatter emits them.
        let mut frontmatter = preserved_frontmatter.get_untracked().unwrap_or_default();
        for (key, value) in extra_frontmatter.get_untracked() {
            frontmatter.insert(key, serde_yaml::Value::String(value));
        }
        if is_work_item {
            use serde_yaml::Value;
            let state_val = wi_state.get_untracked();
            frontmatter.insert("status".into(), Value::String(state_val));
            frontmatter.remove("state");
            let sor_val = wi_system_of_record.get_untracked();
            frontmatter.insert("system_of_record".into(), Value::String(sor_val));
            let assignees: Vec<Value> = wi_assignees
                .get_untracked()
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .map(Value::String)
                .collect();
            frontmatter.insert("assignees".into(), Value::Sequence(assignees));
        }
        let merged_frontmatter = if is_work_item
            || !extra_frontmatter.with_untracked(|fields| fields.is_empty())
            || preserved_frontmatter.with_untracked(|p| p.is_some())
        {
            Some(frontmatter)
        } else {
            None
        };

        let _payload = crate::knowledge::types::BrainFilePayload {
            target: Some(active_target.get_value()),
            node_type: nt,
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
            preserved_frontmatter: merged_frontmatter,
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
                    Ok(result) => {
                        status_msg.set(match result.mode.clone() {
                            WriteMode::Direct => {
                                let saved_path = result.path.clone();
                                edit_path.set(Some(saved_path.clone()));
                                if let Some(fresh_sha) = result.sha.clone() {
                                    edit_sha.set(Some(fresh_sha));
                                }
                                if updating {
                                    format!("Updated: {}", saved_path)
                                } else {
                                    format!("Created: {}", saved_path)
                                }
                            }
                            WriteMode::PullRequest => {
                                format!(
                                    "Proposed via PR #{}: {}",
                                    result
                                        .pr_number
                                        .map(|n| n.to_string())
                                        .unwrap_or_else(|| "?".to_string()),
                                    result.path
                                )
                            }
                        });
                        saving.set(false);
                        if let Some(key) = draft_key_snapshot {
                            draft::clear(&key);
                        }
                        if result.mode == WriteMode::Direct {
                            graph_version.update(|v| *v += 1);
                        }
                    }
                    Err(e) => {
                        // Typed boundary error: surface the actionable message
                        // (stale → reload, no write → PR, rate-limit → retry)
                        // instead of a flattened diagnostic string.
                        status_msg.set(e.actionable_message());
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
        let current_type = config.with_value(|c| c.by_directory(dir).map(|s| s.name.clone()))?;
        let target = node_type.get();
        if current_type == target
            || config.with_value(|c| {
                c.lookup(&target)
                    .map(|s| s.directory.is_empty())
                    .unwrap_or(true)
            })
        {
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
        let target_dir = config.with_value(|c| {
            c.lookup(&to)
                .map(|s| s.directory.clone())
                .unwrap_or_default()
        });
        let new_path = format!("{}/{}", target_dir, filename);

        moving.set(true);
        move_error.set(String::new());
        #[cfg(not(feature = "ssr"))]
        {
            use crate::api::rename_brain_file;
            let active_target = active_target.get_value();
            leptos::task::spawn_local(async move {
                match rename_brain_file(active_target, old_path, new_path.clone(), sha, None).await
                {
                    Ok(res) => {
                        if res.write.mode == WriteMode::Direct {
                            edit_path.set(Some(res.new_path.clone()));
                            // sha is stale after the move (create + delete commits);
                            // exit edit mode so the user reopens with a fresh sha.
                            edit_mode.set(EditMode::Closed);
                            graph_version.update(|v| *v += 1);
                        } else {
                            move_error.set(format!(
                                "Move proposed via PR #{}.",
                                res.write
                                    .pr_number
                                    .map(|n| n.to_string())
                                    .unwrap_or_else(|| "?".to_string())
                            ));
                        }
                        moving.set(false);
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

            <Show when=move || frontmatter_malformed.get()>
                <div class="px-3 py-2 rounded-md bg-rose-500/10 border border-rose-400/40 text-rose-100 text-xs">
                    "This file's YAML frontmatter failed to parse. Saves are disabled to avoid overwriting custom fields. Fix the file on GitHub and reload."
                </div>
            </Show>

            <FrontmatterFields node_type=node_type title=title author=author config=config.get_value() />
            <ExtraFrontmatterFields fields=extra_frontmatter />

            <Show when=move || config.with_value(|c| {
                c.lookup(&node_type.get()).map(|s| s.is_work_item()).unwrap_or(false)
            })>
                <WorkItemFields
                    wi_state=wi_state
                    wi_system_of_record=wi_system_of_record
                    wi_assignees=wi_assignees
                />
            </Show>

            <Show when=move || mismatch.with(|m| m.is_some())>
                {
                    let do_move = do_move;
                    view! {
                        <div class="px-3 py-2 rounded-md bg-amber-500/10 border border-amber-400/40 text-amber-100 text-xs space-y-2">
                                {
                                    let config = config.get_value();
                                    move || mismatch.with(|m| m.as_ref().map(|(path, from, to)| {
                                        let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                                        let from_label = config.lookup(from).map(|s| s.label.clone()).unwrap_or_default();
                                        let to_label = config.lookup(to).map(|s| s.label.clone()).unwrap_or_default();
                                        let to_dir = config.lookup(to).map(|s| s.directory.clone()).unwrap_or_default();
                                        format!(
                                            "This {} lives in `{}/` (the {} folder). Move to `{}/`?",
                                            to_label, dir, from_label, to_dir,
                                        )
                                    }).unwrap_or_default())
                                }
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

            <LocationPicker
                folder=folder
                node_type=node_type
                all_folders=all_folders
                path_preview=markdown_file_path.into()
                is_edit=is_edit
                config=config.get_value()
            />
            <TagInput tags=tags all_tags=all_tags_stored />
            <MarkdownPreview
                active_target=active_target.get_value()
                node_type=node_type.into()
                body=body
                file_path=markdown_file_path.into()
                config=config.get_value()
            />
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
                    class="w-full px-4 py-2 rounded-md bg-teal-500 hover:bg-teal-400 text-slate-950 text-sm font-semibold transition-colors focus:outline-none focus:ring-2 focus:ring-teal-300 disabled:opacity-50 disabled:cursor-not-allowed disabled:hover:bg-teal-500"
                    disabled=move || saving.get() || title.with(|t| t.is_empty()) || frontmatter_malformed.get()
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

fn work_item_status_from_frontmatter(frontmatter: &BTreeMap<String, serde_yaml::Value>) -> String {
    frontmatter
        .get("status")
        .or_else(|| frontmatter.get("state"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("todo")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_item_status_prefers_status_over_legacy_state() {
        let mut frontmatter = BTreeMap::new();
        frontmatter.insert("status".into(), serde_yaml::Value::String("done".into()));
        frontmatter.insert(
            "state".into(),
            serde_yaml::Value::String("in-progress".into()),
        );

        assert_eq!(work_item_status_from_frontmatter(&frontmatter), "done");
    }

    #[test]
    fn work_item_status_keeps_legacy_state_fallback() {
        let mut frontmatter = BTreeMap::new();
        frontmatter.insert("state".into(), serde_yaml::Value::String("blocked".into()));

        assert_eq!(work_item_status_from_frontmatter(&frontmatter), "blocked");
    }
}
