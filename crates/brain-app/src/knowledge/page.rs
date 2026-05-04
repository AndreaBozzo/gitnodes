use std::collections::{HashMap, HashSet};

use leptos::prelude::*;
use leptos_router::NavigateOptions;
use leptos_router::hooks::{use_navigate, use_query_map};

use super::detail_bar::DetailBar;
use super::detail_panel::DetailPanel;
use super::editor::EditorPanel;
use super::filter_panel::FilterPanel;
use super::graph_canvas::GraphCanvas;
use super::live_sync::SyncStatus;
use super::orphan_banner::OrphanBanner;
use super::types::{Edge, EditMode, Node};
use crate::api::{
    AppConfig, FileQueryFilters, RepoFile, list_brain_files, load_brain_config, load_brain_graph,
    refresh_brain_graph,
};
use crate::app::{GraphVersion, SyncStatusSignal};
use brain_domain::{TargetRef, encode_path_segment};

#[component]
pub fn KnowledgePage() -> impl IntoView {
    // Both signals are owned by `App` and shared via context so the global
    // SyncStatusBanner stays in sync with this page's data fetches.
    let graph_version = expect_context::<GraphVersion>().0;
    let sync_status = expect_context::<SyncStatusSignal>().0;
    // Single sequential resource: graph first (triggers bootstrap if cold),
    // then config + files. Prevents the race where list_brain_files queries
    // an empty DB while load_brain_graph's bootstrap is still in progress.
    let data = Resource::new_blocking(
        move || graph_version.get(),
        |_| async {
            #[cfg(feature = "ssr")]
            {
                tokio::time::timeout(std::time::Duration::from_secs(10), async {
                    let graph = load_brain_graph().await?;
                    let config = load_brain_config().await?;
                    let files = list_brain_files(FileQueryFilters::default()).await?;
                    Ok::<_, ServerFnError>((graph, config, files))
                })
                .await
                .map_err(|_| ServerFnError::new("upstream timeout – try refreshing"))?
            }
            #[cfg(not(feature = "ssr"))]
            {
                let graph = load_brain_graph().await?;
                let config = load_brain_config().await?;
                let files = list_brain_files(FileQueryFilters::default()).await?;
                Ok::<_, ServerFnError>((graph, config, files))
            }
        },
    );
    let app_config = expect_context::<Resource<Result<AppConfig, ServerFnError>>>();

    view! {
        <Suspense fallback=|| view! {
            <div class="min-h-screen flex items-center justify-center bg-slate-950 text-slate-400 text-sm">
                "Loading knowledge graph…"
            </div>
        }>
            {move || {
                let d = data.get();
                let a = app_config.get();
                match (d, a) {
                    (Some(Ok(((nodes, edges), cfg, files))), Some(Ok(app))) => {
                        KnowledgeView(KnowledgeViewProps {
                            nodes,
                            edges,
                            files,
                            config: cfg,
                            graph_version,
                            sync_status,
                            target_ref: TargetRef::from(app.target),
                        }).into_any()
                    }
                    (Some(Err(e)), _) | (_, Some(Err(e))) => view! {
                        <div class="min-h-screen flex items-center justify-center bg-slate-950 text-rose-300 text-sm">
                            {format!("Failed to load graph/config: {e}")}
                        </div>
                    }.into_any(),
                    _ => view! { <div></div> }.into_any(),
                }
            }}
        </Suspense>
    }
}

/// Public so `KnowledgePageForTarget` in `brain_switcher.rs` can reuse the
/// same view with a different data source. All URL navigation is rooted at
/// `base_path` so the multi-tenant route gets correct `/{org}/{repo}/knowledge`
/// links instead of `/knowledge`.
#[component]
pub(crate) fn KnowledgeView(
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    files: Vec<RepoFile>,
    config: brain_domain::BrainConfig,
    graph_version: RwSignal<u64>,
    sync_status: RwSignal<SyncStatus>,
    target_ref: TargetRef,
) -> impl IntoView {
    provide_context(target_ref.clone());
    let query = use_query_map();
    let nodes = StoredValue::new(nodes);
    let repo_files = StoredValue::new(files);
    let config = StoredValue::new(config);
    let edges = StoredValue::new(edges);
    let path_to_id: StoredValue<HashMap<String, u32>> = StoredValue::new(
        nodes.with_value(|ns| ns.iter().map(|n| (n.path.clone(), n.id)).collect()),
    );

    // Tag filtering is case-insensitive: collapse case variants into one
    // lowercase canonical form both in the filter vocabulary and when
    // matching against a node's tags.
    let all_tags: Vec<String> = {
        let mut set: HashSet<String> = HashSet::new();
        nodes.with_value(|ns| {
            for n in ns {
                for t in &n.tags {
                    set.insert(t.to_lowercase());
                }
            }
        });
        let mut v: Vec<String> = set.into_iter().collect();
        v.sort();
        v
    };

    let type_counts: HashMap<String, usize> = config
        .with_value(|c| c.node_types.clone())
        .iter()
        .map(|spec| {
            let count =
                nodes.with_value(|ns| ns.iter().filter(|n| n.node_type == spec.name).count());
            (spec.name.clone(), count)
        })
        .collect();
    let total_nodes: usize = type_counts.values().sum();
    // Header overflow: show top N by count, fold the rest into a dropdown.
    let mut nonzero: Vec<(String, usize)> = type_counts
        .iter()
        .filter(|(_, c)| **c > 0)
        .map(|(t, c)| (t.clone(), *c))
        .collect();
    nonzero.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    const HEADER_TOP_N: usize = 5;
    let (header_top, header_rest): (Vec<_>, Vec<_>) = nonzero
        .into_iter()
        .enumerate()
        .partition(|(i, _)| *i < HEADER_TOP_N);
    let header_top: Vec<(String, usize)> = header_top.into_iter().map(|(_, p)| p).collect();
    let header_rest: Vec<(String, usize)> = header_rest.into_iter().map(|(_, p)| p).collect();

    let active_tags = RwSignal::new(HashSet::<String>::new());
    let active_types = RwSignal::new(HashSet::<String>::new());
    let active_path_prefix = RwSignal::new(Option::<String>::None);
    let active_orphan_filter = RwSignal::new(false);
    let hovered = RwSignal::new(None::<u32>);
    let selected = RwSignal::new(None::<u32>);
    let selected_path = RwSignal::new(None::<String>);
    let edit_mode = RwSignal::new(EditMode::Closed);
    let editing = Memo::new(move |_| !matches!(edit_mode.get(), EditMode::Closed));

    // Tags are stored lowercase already (case-insensitive matching); types
    // preserve case because they map to `node_types[].name` in BrainConfig.
    fn parse_csv(raw: &str) -> HashSet<String> {
        raw.split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect()
    }

    fn join_sorted(set: &HashSet<String>) -> String {
        let mut v: Vec<&str> = set.iter().map(String::as_str).collect();
        v.sort();
        v.join(",")
    }

    // Minimal percent-encoder: anything outside RFC 3986 unreserved + a few
    // path-safe symbols gets encoded. We don't need a full crate for this —
    // tags/types are ASCII CSV; paths are repo-relative markdown filenames.
    fn url_encode(input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        for byte in input.bytes() {
            let safe = byte.is_ascii_alphanumeric()
                || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/' | b',');
            if safe {
                out.push(byte as char);
            } else {
                out.push_str(&format!("%{:02X}", byte));
            }
        }
        out
    }

    fn normalize_path_prefix(input: &str) -> String {
        let trimmed = input.trim().trim_matches('/');
        if trimmed.is_empty() {
            String::new()
        } else {
            format!("{trimmed}/")
        }
    }

    Effect::new(move |_| {
        let params = query.get();
        if let Some(path) = params.get_str("path") {
            selected_path.set(Some(path.to_string()));
        }
        let next_tags = params
            .get_str("tags")
            .map(|raw| parse_csv(&raw.to_lowercase()))
            .unwrap_or_default();
        let next_types = params.get_str("types").map(parse_csv).unwrap_or_default();
        let next_path_prefix = params
            .get_str("path_prefix")
            .map(normalize_path_prefix)
            .filter(|raw| !raw.is_empty());
        let next_orphan_filter = params
            .get_str("orphan")
            .is_some_and(|raw| raw == "true" || raw == "1");
        if next_tags != active_tags.get_untracked() {
            active_tags.set(next_tags);
        }
        if next_types != active_types.get_untracked() {
            active_types.set(next_types);
        }
        if next_path_prefix != active_path_prefix.get_untracked() {
            active_path_prefix.set(next_path_prefix);
        }
        if next_orphan_filter != active_orphan_filter.get_untracked() {
            active_orphan_filter.set(next_orphan_filter);
        }
    });

    // Canonical base path for this view: `/knowledge` for the legacy route,
    // `/{org}/{repo}/knowledge` for the multi-tenant route.
    let base_path = format!(
        "/{}/{}/{}/knowledge",
        target_ref.org,
        target_ref.repo,
        encode_path_segment(&target_ref.branch)
    );

    // Push filter changes back to the URL so refresh and link sharing both
    // round-trip through the same query string. `replace=true` keeps filter
    // toggling out of the back/forward history (one filter change = one nav
    // event would make Back unusable on this page).
    let navigate = use_navigate();
    let base_path_nav = base_path.clone();
    Effect::new(move |_| {
        let tags = active_tags.get();
        let types = active_types.get();
        let path = selected_path.get();
        let path_prefix = active_path_prefix.get();
        let orphan_filter = active_orphan_filter.get();
        let mut parts: Vec<String> = Vec::new();
        if let Some(p) = path.as_ref().filter(|s| !s.is_empty()) {
            parts.push(format!("path={}", url_encode(p)));
        }
        if !tags.is_empty() {
            parts.push(format!("tags={}", url_encode(&join_sorted(&tags))));
        }
        if !types.is_empty() {
            parts.push(format!("types={}", url_encode(&join_sorted(&types))));
        }
        if let Some(prefix) = path_prefix.as_ref().filter(|s| !s.is_empty()) {
            parts.push(format!(
                "path_prefix={}",
                url_encode(prefix.trim_end_matches('/'))
            ));
        }
        if orphan_filter {
            parts.push("orphan=true".to_string());
        }
        let target = if parts.is_empty() {
            base_path_nav.clone()
        } else {
            format!("{}?{}", base_path_nav, parts.join("&"))
        };
        // Avoid feedback loop: only navigate if the URL actually differs.
        let current = query.get_untracked();
        let current_tags = current
            .get_str("tags")
            .map(|raw| parse_csv(&raw.to_lowercase()))
            .unwrap_or_default();
        let current_types = current.get_str("types").map(parse_csv).unwrap_or_default();
        let current_path = current.get_str("path").map(str::to_string);
        let current_path_prefix = current
            .get_str("path_prefix")
            .map(normalize_path_prefix)
            .filter(|raw| !raw.is_empty());
        let current_orphan_filter = current
            .get_str("orphan")
            .is_some_and(|raw| raw == "true" || raw == "1");
        if current_tags == tags
            && current_types == types
            && current_path == path
            && current_path_prefix == path_prefix
            && current_orphan_filter == orphan_filter
        {
            return;
        }
        navigate(
            &target,
            NavigateOptions {
                replace: true,
                ..Default::default()
            },
        );
    });

    Effect::new(move |_| {
        let next = selected_path
            .get()
            .and_then(|path| path_to_id.with_value(|map| map.get(&path).copied()));
        if next != selected.get_untracked() {
            selected.set(next);
        }
    });

    // Esc cascade: close the editor first if it's open, otherwise clear the
    // selected node. One-key dismiss for the frontmost overlay. Handler runs
    // only after hydration; SSR has no `window`.
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::JsCast;
        use wasm_bindgen::closure::Closure;
        use web_sys::KeyboardEvent;

        if let Some(window) = web_sys::window() {
            let handler = Closure::<dyn FnMut(KeyboardEvent)>::new(move |ev: KeyboardEvent| {
                if ev.key() != "Escape" {
                    return;
                }
                // Don't hijack Esc while the user is typing in a form control —
                // textarea/input expect Esc to stay local (e.g. clearing IME).
                let typing_in_form = web_sys::window()
                    .and_then(|w| w.document())
                    .and_then(|d| d.active_element())
                    .map(|el| {
                        let tag = el.tag_name();
                        tag.eq_ignore_ascii_case("input")
                            || tag.eq_ignore_ascii_case("textarea")
                            || tag.eq_ignore_ascii_case("select")
                            || el.get_attribute("contenteditable").is_some()
                    })
                    .unwrap_or(false);
                if typing_in_form {
                    return;
                }
                if !matches!(edit_mode.get_untracked(), EditMode::Closed) {
                    edit_mode.set(EditMode::Closed);
                    ev.prevent_default();
                    return;
                }
                if selected_path.get_untracked().is_some() {
                    selected_path.set(None);
                    ev.prevent_default();
                }
            });
            let _ = window
                .add_event_listener_with_callback("keydown", handler.as_ref().unchecked_ref());
            // Keep the closure alive for the lifetime of the component; drop
            // on route change so we don't leak a listener per remount.
            let stored = StoredValue::new_local(handler);
            on_cleanup(move || {
                stored.with_value(|h| {
                    if let Some(w) = web_sys::window() {
                        let _ = w.remove_event_listener_with_callback(
                            "keydown",
                            h.as_ref().unchecked_ref(),
                        );
                    }
                });
            });
        }
    }

    let visible_ids = Memo::new(move |_| {
        let tags = active_tags.get();
        let types = active_types.get();
        let path_prefix = active_path_prefix.get();
        let orphan_filter = active_orphan_filter.get();
        let orphan_paths: HashSet<String> = if orphan_filter {
            repo_files.with_value(|files| {
                files
                    .iter()
                    .filter(|file| file.is_orphan_in_graph)
                    .map(|file| file.path.clone())
                    .collect()
            })
        } else {
            HashSet::new()
        };
        nodes.with_value(|ns| {
            ns.iter()
                .filter(|n| types.is_empty() || types.contains(&n.node_type))
                .filter(|n| {
                    tags.is_empty() || n.tags.iter().any(|t| tags.contains(&t.to_lowercase()))
                })
                .filter(|n| {
                    path_prefix
                        .as_deref()
                        .is_none_or(|prefix| n.path.starts_with(prefix))
                })
                .filter(|n| !orphan_filter || orphan_paths.contains(&n.path))
                .map(|n| n.id)
                .collect::<HashSet<u32>>()
        })
    });

    let node_titles: Vec<(String, String)> = nodes.with_value(|ns| {
        ns.iter()
            .filter(|n| !n.path.is_empty())
            .map(|n| (n.path.clone(), n.title.clone()))
            .collect()
    });

    let admin_href = format!(
        "/{}/{}/{}/admin",
        target_ref.org,
        target_ref.repo,
        encode_path_segment(&target_ref.branch)
    );
    let target_label = format!(
        "{}/{}/{}",
        target_ref.org, target_ref.repo, target_ref.branch
    );

    view! {
        <div class="h-screen flex flex-col bg-slate-950 text-slate-100">
            <header class="px-6 py-4 border-b border-slate-800 flex items-center gap-3">
                <div class="w-2 h-2 rounded-full bg-teal-400"></div>
                <h1 class="text-sm font-semibold tracking-wide uppercase text-slate-300">
                    "Brain · Knowledge"
                </h1>
                {(!target_label.is_empty()).then(|| view! {
                    <span class="text-xs text-slate-500 font-mono">{target_label.clone()}</span>
                })}
                <a
                    href=admin_href
                    rel="external"
                    class="text-xs text-slate-500 hover:text-slate-300 ml-2"
                >
                    "· /admin"
                </a>
                <div class="ml-auto flex items-center gap-2 flex-wrap justify-end">
                    <span
                        class="px-2.5 py-1 rounded-md bg-slate-900/80 border border-slate-800 text-[10px] uppercase tracking-widest text-slate-400"
                        title="Total nodes in the current graph"
                    >
                        <span class="text-slate-200 font-semibold tabular-nums">{total_nodes}</span>
                        " nodes"
                    </span>
                    {
                        let config_top = config.get_value();
                        header_top.into_iter().map(move |(t_name, count)| {
                            let spec = config_top.lookup(&t_name).unwrap_or(config_top.default_spec());
                            view! {
                                <span class="px-2.5 py-1 rounded-md bg-slate-900/80 border border-slate-800 text-[10px] uppercase tracking-widest text-slate-400">
                                    <span class="text-slate-200 font-semibold tabular-nums">{count}</span>
                                    " "
                                    <span>{spec.label.clone()}</span>
                                </span>
                            }
                        }).collect_view()
                    }
                    {
                        let rest = header_rest.clone();
                        (!rest.is_empty()).then(|| {
                            let config_rest = config.get_value();
                            let extra = rest.len();
                            view! {
                                <div class="dropdown dropdown-end">
                                    <div
                                        tabindex="0"
                                        role="button"
                                        class="px-2.5 py-1 rounded-md bg-slate-900/80 border border-slate-800 text-[10px] uppercase tracking-widest text-slate-400 hover:text-slate-200 cursor-pointer"
                                        title="Show remaining types"
                                    >
                                        "+" {extra} " more"
                                    </div>
                                    <ul
                                        tabindex="0"
                                        class="dropdown-content menu menu-sm z-10 mt-1 p-2 shadow-lg bg-slate-900 border border-slate-800 rounded-md min-w-[180px]"
                                    >
                                        {rest.into_iter().map(move |(t_name, count)| {
                                            let spec = config_rest.lookup(&t_name).unwrap_or(config_rest.default_spec());
                                            view! {
                                                <li>
                                                    <span class="flex items-center justify-between gap-3 text-[11px] text-slate-300">
                                                        <span>{spec.label.clone()}</span>
                                                        <span class="tabular-nums text-slate-400">{count}</span>
                                                    </span>
                                                </li>
                                            }
                                        }).collect_view()}
                                    </ul>
                                </div>
                            }
                        })
                    }
                    <RefreshButton graph_version=graph_version sync_status=sync_status />
                    <button
                        class="btn btn-primary btn-outline btn-xs ml-2"
                        on:click=move |_| {
                            edit_mode.update(|m| {
                                *m = if matches!(m, EditMode::Closed) {
                                    EditMode::New
                                } else {
                                    EditMode::Closed
                                };
                            });
                        }
                    >
                        {move || if editing.get() { "Close Editor" } else { "+ New" }}
                    </button>
                </div>
            </header>
            <OrphanBanner nodes=nodes config=config />
            <div class="flex-1 flex min-h-0">
                <FilterPanel
                    all_tags=all_tags.clone()
                    active_tags=active_tags
                    active_types=active_types
                    active_path_prefix=active_path_prefix
                    active_orphan_filter=active_orphan_filter
                    selected_path=selected_path
                    repo_files=repo_files.get_value()
                    config=config.get_value()
                    type_counts=type_counts.clone()
                    current_org=target_ref.org.clone()
                    current_repo=target_ref.repo.clone()
                    current_branch=target_ref.branch.clone()
                />
                <Show when=move || editing.get()>
                    <EditorPanel
                        node_titles=node_titles.clone()
                        all_tags=all_tags.clone()
                        edit_mode=edit_mode
                        graph_version=graph_version
                        config=config.get_value()
                    />
                </Show>
                <GraphCanvas
                    nodes=nodes
                    edges=edges
                    visible_ids=visible_ids.into()
                    hovered=hovered
                    selected=selected
                    selected_path=selected_path
                    config=config.get_value()
                />
                <DetailPanel
                    nodes=nodes
                    edges=edges
                    selected=selected
                    selected_path=selected_path
                    active_path_prefix=active_path_prefix
                    edit_mode=edit_mode
                    graph_version=graph_version
                    config=config.get_value()
                />
            </div>
            <DetailBar
                nodes=nodes
                edges=edges
                hovered=hovered.into()
                selected=selected.into()
                config=config.get_value()
            />
        </div>
    }
}

/// Rebuilds the server-side per-target SQLite projection and bumps
/// `graph_version` so the `Resource` re-reads the refreshed snapshot.
#[component]
fn RefreshButton(graph_version: RwSignal<u64>, sync_status: RwSignal<SyncStatus>) -> impl IntoView {
    let busy = RwSignal::new(false);
    let target = expect_context::<TargetRef>();
    view! {
        <button
            class="btn btn-ghost btn-xs"
            title="Rebuild the local graph projection from the repo."
            disabled=move || busy.get()
            on:click=move |_| {
                if busy.get_untracked() {
                    return;
                }
                busy.set(true);
                let target = target.clone();
                leptos::task::spawn_local(async move {
                    match refresh_brain_graph(target).await {
                        Ok(()) => {
                            graph_version.update(|v| *v += 1);
                            sync_status.set(SyncStatus::Fresh);
                        }
                        Err(error) => {
                            sync_status.set(SyncStatus::Stale {
                                message: Some(format!(
                                    "Manual refresh failed: {error}. Showing the last successful snapshot."
                                )),
                            });
                        }
                    }
                    busy.set(false);
                });
            }
        >
            {move || if busy.get() { "Refreshing…" } else { "Refresh" }}
        </button>
    }
}
