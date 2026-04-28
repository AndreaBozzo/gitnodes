use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

use brain_domain::{BrainConfig, ViewSpec, slugify_view_name};

use crate::api::{WriteMode, list_views, load_brain_config, save_views};

#[component]
pub fn ViewsAdminPage() -> impl IntoView {
    let params = use_params_map();
    let target_prefix = Memo::new(move |_| {
        let (org, repo) = params.with(|p| {
            (
                p.get("org").unwrap_or_default().to_string(),
                p.get("repo").unwrap_or_default().to_string(),
            )
        });
        if org.is_empty() || repo.is_empty() {
            String::new()
        } else {
            format!("/{org}/{repo}")
        }
    });
    let reload_tick = RwSignal::new(0u32);
    let outcome_msg = RwSignal::new(Option::<OutcomeBanner>::None);

    let initial = Resource::new_blocking(
        move || reload_tick.get(),
        |_| async move {
            let cfg = load_brain_config().await?;
            let views = list_views().await?;
            Ok::<_, leptos::prelude::ServerFnError>((cfg, views))
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
                        Ok((cfg, views)) => view! {
                            <ViewsEditor
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

    let save = Action::new(move |payload: &Vec<ViewSpec>| {
        let payload = payload.clone();
        async move { save_views(payload).await }
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
            outcome_msg.set(Some(banner));
            reload_tick.update(|t| *t += 1);
        } else if let Some(Err(e)) = save.value().get() {
            outcome_msg.set(Some(OutcomeBanner::Error(e.to_string())));
        }
    });

    let pending = save.pending();

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
                save.dispatch(specs);
            }
            Err(msg) => outcome_msg.set(Some(OutcomeBanner::Error(msg))),
        }
    };

    view! {
        <div class="space-y-4">
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
                    {move || if pending.get() { "saving…" } else { "save views" }}
                </button>
            </div>
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
                    class="ml-auto text-[11px] text-rose-300 hover:text-rose-200"
                    on:click=on_remove
                >
                    "remove"
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
