use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

use gitnodes_domain::{BrainConfig, TargetRef, ViewSpec, decode_path_segment, slugify_view_name};

use crate::api::{
    ViewsPreview, WriteMode, list_views, load_brain_config_for_target, preview_views,
    resolve_legacy_target, save_views,
};

#[component]
pub fn ViewsAdminPage() -> impl IntoView {
    let params = use_params_map();
    let target_prefix = Memo::new(move |_| {
        let (org, repo, branch) = params.with(|p| {
            (
                p.get("org").unwrap_or_default().to_string(),
                p.get("repo").unwrap_or_default().to_string(),
                p.get("branch").unwrap_or_default().to_string(),
            )
        });
        if org.is_empty() || repo.is_empty() {
            String::new()
        } else if branch.is_empty() {
            format!("/{org}/{repo}")
        } else {
            format!("/{org}/{repo}/{branch}")
        }
    });
    let target = Memo::new(move |_| {
        params.with(|p| {
            (
                p.get("org").unwrap_or_default().to_string(),
                p.get("repo").unwrap_or_default().to_string(),
                p.get("branch").unwrap_or_default().to_string(),
            )
        })
    });
    let reload_tick = RwSignal::new(0u32);
    let outcome_msg = RwSignal::new(Option::<OutcomeBanner>::None);

    let initial = Resource::new_blocking(
        move || (reload_tick.get(), target.get()),
        |(_, (org, repo, branch))| async move {
            let target = if org.is_empty() || repo.is_empty() {
                let app = crate::api::get_app_config().await?;
                TargetRef::from(app.target)
            } else if branch.is_empty() {
                resolve_legacy_target(org, repo).await?
            } else {
                TargetRef::new(org, repo, decode_path_segment(&branch))
            };
            let cfg = load_brain_config_for_target(target.clone()).await?;
            let views = list_views(target.clone()).await?;
            Ok::<_, crate::api::ApiError>((target, cfg, views))
        },
    );

    view! {
        <div class="min-h-screen bg-slate-950 text-slate-100">
            <header class="px-6 py-4 border-b border-slate-800 flex items-center gap-3">
                <div class="w-2 h-2 rounded-full bg-amber-400"></div>
                <h1 class="text-sm font-semibold tracking-wide uppercase text-slate-300">
                    "Brain · Admin · Views"
                </h1>
                <span class="text-xs text-slate-500 ml-2">
                    {move || format!("{}/admin/views", target_prefix.get())}
                </span>
                <a
                    href=move || format!("{}/admin", target_prefix.get())
                    rel="external"
                    class="ml-auto text-xs text-slate-400 hover:text-slate-200"
                >
                    "← back to admin"
                </a>
            </header>

            <main class="p-6 space-y-6 max-w-3xl mx-auto">
                <p class="text-xs text-slate-400 leading-relaxed">
                    "Saved views are named filter sets shown as shortcuts in the Knowledge sidebar. "
                    "Each view applies the same "<code class="font-mono text-slate-300">"?tags="</code>
                    " / "<code class="font-mono text-slate-300">"?types="</code>
                    " filters that already exist — nothing new is introduced. "
                    "Saving will rewrite "<code class="font-mono text-slate-300">".brain-config.yml"</code>
                    " in the target repo, going through the same permission-aware orchestrator as document edits."
                </p>

                {move || outcome_msg.get().map(|banner| view! { <OutcomeBannerView banner=banner /> })}

                <Suspense fallback=|| view! {
                    <p class="text-xs text-slate-500">"loading…"</p>
                }>
                    {move || initial.get().map(|res| match res {
                        Ok((target, cfg, views)) => view! {
                            <ViewsEditor
                                target=target
                                cfg=cfg
                                initial_views=views
                                outcome_msg=outcome_msg
                                reload_tick=reload_tick
                            />
                        }.into_any(),
                        Err(e) => view! {
                            <p class="text-xs text-rose-300">{format!("failed to load views: {e}")}</p>
                        }.into_any(),
                    })}
                </Suspense>
            </main>
        </div>
    }
}

#[component]
fn ViewsEditor(
    target: TargetRef,
    cfg: BrainConfig,
    initial_views: Vec<ViewSpec>,
    outcome_msg: RwSignal<Option<OutcomeBanner>>,
    reload_tick: RwSignal<u32>,
) -> impl IntoView {
    let drafts: RwSignal<Vec<ViewDraft>> = RwSignal::new(
        initial_views
            .into_iter()
            .map(ViewDraft::from_spec)
            .collect(),
    );
    let type_options: Vec<String> = cfg.node_types.iter().map(|t| t.name.clone()).collect();
    let preview_state = RwSignal::new(Option::<(ViewsPreview, Vec<ViewSpec>)>::None);
    let preview_target = target.clone();
    let preview = Action::new(move |payload: &Vec<ViewSpec>| {
        let payload = payload.clone();
        let target = preview_target.clone();
        async move {
            let plan = preview_views(target, payload.clone()).await?;
            Ok::<_, crate::api::ApiError>((plan, payload))
        }
    });
    let save = Action::new(move |payload: &(Vec<ViewSpec>, Option<String>)| {
        let (views, expected_sha) = payload.clone();
        let target = target.clone();
        async move { save_views(target, views, expected_sha).await }
    });

    Effect::new(move |_| {
        if let Some(Ok(result)) = preview.value().get() {
            preview_state.set(Some(result));
            outcome_msg.set(None);
        } else if let Some(Err(error)) = preview.value().get() {
            outcome_msg.set(Some(OutcomeBanner::Error(error.to_string())));
        }
    });

    Effect::new(move |_| {
        if let Some(Ok(result)) = save.value().get() {
            let banner = match result.mode {
                WriteMode::Direct => OutcomeBanner::Saved,
                WriteMode::PullRequest => OutcomeBanner::ProposedViaPr {
                    number: result.pr_number,
                    url: result.pr_url,
                },
            };
            preview_state.set(None);
            outcome_msg.set(Some(banner));
            reload_tick.update(|t| *t += 1);
        } else if let Some(Err(e)) = save.value().get() {
            preview_state.set(None);
            outcome_msg.set(Some(OutcomeBanner::Error(e.to_string())));
        }
    });

    let preview_pending = preview.pending();
    let save_pending = save.pending();
    let pending = Memo::new(move |_| preview_pending.get() || save_pending.get());
    let draft_count = Memo::new(move |_| drafts.with(Vec::len));

    let on_add = move |_| {
        drafts.update(|list| {
            list.push(ViewDraft::empty());
        });
    };

    let on_save = move |_| {
        let collected: Result<Vec<ViewSpec>, String> =
            drafts.with(|list| list.iter().map(ViewDraft::to_spec).collect());
        match collected {
            Ok(specs) => {
                outcome_msg.set(None);
                preview.dispatch(specs);
            }
            Err(msg) => outcome_msg.set(Some(OutcomeBanner::Error(msg))),
        }
    };

    view! {
        <div class="space-y-4">
            <fieldset prop:disabled=move || pending.get() || preview_state.get().is_some() class="space-y-4 disabled:opacity-60">
                <Show when=move || draft_count.get() == 0>
                    <div class="rounded-md border border-amber-400/30 bg-amber-500/10 px-4 py-3 text-xs text-amber-100">
                        "No saved views. Saving now will remove the "
                        <code class="font-mono">"views"</code>
                        " block from "
                        <code class="font-mono">".brain-config.yml"</code>
                        "."
                    </div>
                </Show>
                <For
                    each=move || drafts.with(|list| list.iter().map(|d| d.id).collect::<Vec<_>>())
                    key=|id| *id
                    children=move |id| {
                        let type_options = type_options.clone();
                        view! {
                            <ViewDraftCard
                                draft_id=id
                                drafts=drafts
                                type_options=type_options
                            />
                        }
                    }
                />
                <div class="flex items-center gap-3 pt-2">
                    <button
                        class="px-3 py-1.5 rounded-md bg-slate-800 hover:bg-slate-700 text-xs text-slate-200 border border-slate-700"
                        on:click=on_add
                    >
                        "+ add view"
                    </button>
                    <button
                        class="ml-auto px-3 py-1.5 rounded-md bg-teal-500/20 border border-teal-400/40 text-teal-100 hover:bg-teal-500/30 text-xs disabled:opacity-50"
                        on:click=on_save
                        prop:disabled=move || pending.get()
                    >
                        {move || {
                            if preview_pending.get() {
                                "planning…"
                            } else if draft_count.get() == 0 {
                                "preview deletion"
                            } else {
                                "preview changes"
                            }
                        }}
                    </button>
                </div>
            </fieldset>

            {move || preview_state.get().map(|(plan, specs)| {
                let expected_sha = plan.expected_sha.clone();
                view! {
                    <div class="rounded-md border border-teal-400/30 bg-slate-900/70 p-4 space-y-4">
                        <div>
                            <h2 class="text-sm font-semibold text-teal-100">"Review planned config change"</h2>
                            <p class="text-[11px] text-slate-400 font-mono">
                                {format!("{} · {} · head {}", plan.operation, plan.path, plan.head_sha)}
                            </p>
                        </div>
                        <div class="grid gap-3 lg:grid-cols-2">
                            <div>
                                <p class="mb-1 text-[11px] uppercase tracking-wide text-slate-500">"Before"</p>
                                <pre class="max-h-80 overflow-auto whitespace-pre-wrap rounded bg-slate-950 p-3 text-[11px] text-slate-300 border border-slate-800">{plan.current_yaml}</pre>
                            </div>
                            <div>
                                <p class="mb-1 text-[11px] uppercase tracking-wide text-slate-500">"After"</p>
                                <pre class="max-h-80 overflow-auto whitespace-pre-wrap rounded bg-slate-950 p-3 text-[11px] text-slate-300 border border-slate-800">{plan.proposed_yaml}</pre>
                            </div>
                        </div>
                        <div class="flex justify-end gap-2">
                            <button
                                class="px-3 py-1.5 rounded border border-slate-700 text-xs text-slate-300 hover:bg-slate-800"
                                on:click=move |_| preview_state.set(None)
                                prop:disabled=move || save_pending.get()
                            >
                                "Cancel"
                            </button>
                            <button
                                class="px-3 py-1.5 rounded border border-teal-400/40 bg-teal-500/20 text-xs text-teal-100 hover:bg-teal-500/30 disabled:opacity-50"
                                on:click=move |_| {
                                    save.dispatch((specs.clone(), expected_sha.clone()));
                                }
                                prop:disabled=move || save_pending.get()
                            >
                                {move || if save_pending.get() { "saving…" } else { "confirm save" }}
                            </button>
                        </div>
                    </div>
                }
            })}
        </div>
    }
}

#[component]
fn ViewDraftCard(
    draft_id: u64,
    drafts: RwSignal<Vec<ViewDraft>>,
    type_options: Vec<String>,
) -> impl IntoView {
    // Position of this card's draft in the live list. Resolved on every read so
    // it stays correct after siblings are removed or reordered.
    let position =
        Memo::new(move |_| drafts.with(|list| list.iter().position(|d| d.id == draft_id)));
    let display_idx = Memo::new(move |_| position.get().unwrap_or(0));
    let name_value = Memo::new(move |_| {
        drafts.with(|list| {
            position
                .get()
                .and_then(|i| list.get(i))
                .map(|d| d.name.clone())
                .unwrap_or_default()
        })
    });
    let slug_value = Memo::new(move |_| {
        drafts.with(|list| {
            position
                .get()
                .and_then(|i| list.get(i))
                .map(|d| d.effective_slug())
                .unwrap_or_default()
        })
    });
    let slug_overridden = Memo::new(move |_| {
        drafts.with(|list| {
            position
                .get()
                .and_then(|i| list.get(i))
                .map(|d| d.slug_overridden)
                .unwrap_or(false)
        })
    });
    let tags_csv = Memo::new(move |_| {
        drafts.with(|list| {
            position
                .get()
                .and_then(|i| list.get(i))
                .map(|d| d.tags.join(", "))
                .unwrap_or_default()
        })
    });
    let selected_types: Memo<Vec<String>> = Memo::new(move |_| {
        drafts.with(|list| {
            position
                .get()
                .and_then(|i| list.get(i))
                .map(|d| d.types.clone())
                .unwrap_or_default()
        })
    });

    let on_remove = move |_| {
        drafts.update(|list| {
            if let Some(i) = list.iter().position(|d| d.id == draft_id) {
                list.remove(i);
            }
        });
    };

    view! {
        <div class="border border-slate-800 rounded-md p-4 bg-slate-900/40 space-y-3">
            <div class="flex items-center gap-3">
                <span class="text-[11px] uppercase tracking-widest text-slate-500">{move || format!("view #{}", display_idx.get() + 1)}</span>
                <button
                    class="ml-auto rounded border border-rose-400/30 bg-rose-500/10 px-2 py-0.5 text-[11px] text-rose-200 hover:bg-rose-500/20"
                    on:click=on_remove
                    title="Delete this saved view from the draft list. Click save views to persist the deletion."
                >
                    "delete view"
                </button>
            </div>

            <label class="block text-xs">
                <span class="text-slate-400">"name"</span>
                <input
                    class="mt-1 w-full bg-slate-950 border border-slate-800 rounded px-2 py-1 text-sm text-slate-100"
                    prop:value=move || name_value.get()
                    on:input=move |ev| {
                        let val = event_target_value(&ev);
                        drafts.update(|list| {
                            if let Some(i) = list.iter().position(|d| d.id == draft_id)
                                && let Some(d) = list.get_mut(i)
                            {
                                d.name = val;
                            }
                        });
                    }
                    placeholder="Open tasks"
                />
            </label>

            <label class="block text-xs">
                <span class="text-slate-400 flex items-center gap-2">
                    "slug"
                    <span class="text-[10px] text-slate-500">
                        {move || if slug_overridden.get() { "(manual override)" } else { "(auto from name)" }}
                    </span>
                    <button
                        class="ml-auto text-[10px] text-slate-400 hover:text-slate-200"
                        on:click=move |_| {
                            drafts.update(|list| {
                                if let Some(i) = list.iter().position(|d| d.id == draft_id)
                                    && let Some(d) = list.get_mut(i)
                                {
                                    d.slug_overridden = !d.slug_overridden;
                                    if !d.slug_overridden {
                                        d.slug_manual.clear();
                                    } else if d.slug_manual.is_empty() {
                                        d.slug_manual = slugify_view_name(&d.name);
                                    }
                                }
                            });
                        }
                    >
                        {move || if slug_overridden.get() { "use auto" } else { "override" }}
                    </button>
                </span>
                <input
                    class="mt-1 w-full bg-slate-950 border border-slate-800 rounded px-2 py-1 text-sm text-slate-100 font-mono disabled:opacity-50"
                    prop:value=move || slug_value.get()
                    prop:disabled=move || !slug_overridden.get()
                    on:input=move |ev| {
                        let val = event_target_value(&ev);
                        drafts.update(|list| {
                            if let Some(i) = list.iter().position(|d| d.id == draft_id)
                                && let Some(d) = list.get_mut(i)
                            {
                                d.slug_manual = val;
                            }
                        });
                    }
                />
            </label>

            <label class="block text-xs">
                <span class="text-slate-400">"tags (comma-separated, lowercase)"</span>
                <input
                    class="mt-1 w-full bg-slate-950 border border-slate-800 rounded px-2 py-1 text-sm text-slate-100"
                    prop:value=move || tags_csv.get()
                    on:input=move |ev| {
                        let val = event_target_value(&ev);
                        let parsed: Vec<String> = val
                            .split(',')
                            .map(|s| s.trim().to_lowercase())
                            .filter(|s| !s.is_empty())
                            .collect();
                        drafts.update(|list| {
                            if let Some(i) = list.iter().position(|d| d.id == draft_id)
                                && let Some(d) = list.get_mut(i)
                            {
                                d.tags = parsed;
                            }
                        });
                    }
                    placeholder="urgent, customer"
                />
            </label>

            <div class="block text-xs">
                <span class="text-slate-400">"types"</span>
                <div class="mt-1 flex flex-wrap gap-1.5">
                    {type_options.into_iter().map(|t| {
                        let t_render = t.clone();
                        let t_for_active = t.clone();
                        let t_for_toggle = t.clone();
                        let active = Memo::new(move |_| {
                            selected_types.with(|s| s.contains(&t_for_active))
                        });
                        view! {
                            <button
                                class=move || {
                                    let base = "px-2 py-0.5 rounded text-[11px] border ";
                                    if active.get() {
                                        format!("{base}bg-teal-500/30 border-teal-400/50 text-teal-100")
                                    } else {
                                        format!("{base}bg-slate-800 border-slate-700 text-slate-300")
                                    }
                                }
                                on:click=move |_| {
                                    let t = t_for_toggle.clone();
                                    drafts.update(|list| {
                                        if let Some(i) = list.iter().position(|d| d.id == draft_id)
                                            && let Some(d) = list.get_mut(i)
                                        {
                                            if let Some(pos) = d.types.iter().position(|x| x == &t) {
                                                d.types.remove(pos);
                                            } else {
                                                d.types.push(t);
                                            }
                                        }
                                    });
                                }
                            >
                                {t_render}
                            </button>
                        }
                    }).collect_view()}
                </div>
            </div>
        </div>
    }
}

#[derive(Clone, Debug)]
struct ViewDraft {
    id: u64,
    name: String,
    slug_manual: String,
    slug_overridden: bool,
    tags: Vec<String>,
    types: Vec<String>,
    /// Preserved verbatim through the draft → save roundtrip. The current admin
    /// UI has no widget for editing weight; authors set it by hand in YAML, but
    /// we must round-trip the value so that opening/saving the admin form on a
    /// repo that uses pinned views doesn't silently strip the pins.
    weight: Option<i32>,
}

impl ViewDraft {
    fn from_spec(spec: ViewSpec) -> Self {
        let auto = slugify_view_name(&spec.name);
        let slug_overridden = spec.slug != auto;
        Self {
            id: next_draft_id(),
            name: spec.name,
            slug_manual: if slug_overridden {
                spec.slug.clone()
            } else {
                String::new()
            },
            slug_overridden,
            tags: spec.tags,
            types: spec.types,
            weight: spec.weight,
        }
    }

    fn empty() -> Self {
        Self {
            id: next_draft_id(),
            name: String::new(),
            slug_manual: String::new(),
            slug_overridden: false,
            tags: Vec::new(),
            types: Vec::new(),
            weight: None,
        }
    }

    fn effective_slug(&self) -> String {
        if self.slug_overridden && !self.slug_manual.trim().is_empty() {
            self.slug_manual.trim().to_string()
        } else {
            slugify_view_name(&self.name)
        }
    }

    fn to_spec(&self) -> Result<ViewSpec, String> {
        let name = self.name.trim().to_string();
        if name.is_empty() {
            return Err("every view needs a name".into());
        }
        let slug = self.effective_slug();
        if slug.is_empty() {
            return Err(format!(
                "view {name:?} has no slug — name has no alphanumerics, set one explicitly",
            ));
        }
        Ok(ViewSpec {
            name,
            slug,
            tags: self.tags.clone(),
            types: self.types.clone(),
            weight: self.weight,
        })
    }
}

fn next_draft_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone, Debug)]
enum OutcomeBanner {
    Saved,
    ProposedViaPr {
        number: Option<u64>,
        url: Option<String>,
    },
    Error(String),
}

#[component]
fn OutcomeBannerView(banner: OutcomeBanner) -> impl IntoView {
    match banner {
        OutcomeBanner::Saved => view! {
            <div class="px-3 py-2 rounded-md bg-emerald-500/10 border border-emerald-400/30 text-emerald-200 text-xs">
                "Saved to the target branch."
            </div>
        }
        .into_any(),
        OutcomeBanner::ProposedViaPr { number, url } => {
            let label = match (number, url.as_deref()) {
                (Some(n), Some(u)) => view! {
                    <span>
                        "Proposed via PR "
                        <a class="underline hover:text-amber-100" href=u.to_string() rel="external" target="_blank">
                            {format!("#{n}")}
                        </a>
                        " — direct push not permitted."
                    </span>
                }
                .into_any(),
                _ => view! { <span>"Proposed via PR — direct push not permitted."</span> }.into_any(),
            };
            view! {
                <div class="px-3 py-2 rounded-md bg-amber-500/10 border border-amber-400/30 text-amber-200 text-xs">
                    {label}
                </div>
            }
            .into_any()
        }
        OutcomeBanner::Error(msg) => view! {
            <div class="px-3 py-2 rounded-md bg-rose-500/10 border border-rose-400/30 text-rose-200 text-xs">
                {format!("Save failed: {msg}")}
            </div>
        }
        .into_any(),
    }
}
