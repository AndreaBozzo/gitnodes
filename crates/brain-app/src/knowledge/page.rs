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
    AppConfig, ConfigLoadDiagnostic, FileQueryFilters, RepoFile, SearchBrainQuery, SearchHit,
    list_brain_files, load_brain_config_status, load_brain_graph, refresh_brain_graph,
    search_brain,
};
use crate::app::{GraphVersion, SyncStatusSignal};
use brain_domain::{TargetRef, encode_path_segment};

const MIN_SEARCH_QUERY_CHARS: usize = 2;
#[cfg(feature = "hydrate")]
const SEARCH_DEBOUNCE_MS: u32 = 450;

fn search_query_ready(query: &str) -> bool {
    query.trim().chars().count() >= MIN_SEARCH_QUERY_CHARS
}

fn knowledge_loading_view() -> impl IntoView {
    view! {
        <div class="min-h-screen flex items-center justify-center bg-slate-950 text-slate-300">
            <div class="flex items-center gap-3 rounded-md border border-slate-800 bg-slate-900/70 px-4 py-3 shadow-lg shadow-black/20">
                <span class="h-2 w-2 rounded-full bg-teal-300 animate-brain-pulse"></span>
                <div>
                    <div class="text-xs font-semibold uppercase tracking-widest text-slate-400">
                        "Opening Brain"
                    </div>
                    <div class="mt-0.5 text-sm text-slate-200">
                        "Loading graph, files, and saved views."
                    </div>
                </div>
            </div>
        </div>
    }
}

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
                    let config_status = load_brain_config_status().await?;
                    let files = list_brain_files(FileQueryFilters::default()).await?;
                    Ok::<_, crate::api::ApiError>((graph, config_status, files))
                })
                .await
                .map_err(|_| {
                    crate::api::ApiError::Internal("upstream timeout – try refreshing".into())
                })?
            }
            #[cfg(not(feature = "ssr"))]
            {
                let graph = load_brain_graph().await?;
                let config_status = load_brain_config_status().await?;
                let files = list_brain_files(FileQueryFilters::default()).await?;
                Ok::<_, crate::api::ApiError>((graph, config_status, files))
            }
        },
    );
    let app_config = expect_context::<Resource<Result<AppConfig, crate::api::ApiError>>>();

    view! {
        <Suspense fallback=knowledge_loading_view>
            {move || {
                let d = data.get();
                let a = app_config.get();
                match (d, a) {
                    (Some(Ok(((nodes, edges), config_status, files))), Some(Ok(app))) => {
                        KnowledgeView(KnowledgeViewProps {
                            nodes,
                            edges,
                            files,
                            config: config_status.config,
                            config_diagnostic: config_status.diagnostic,
                            graph_version,
                            sync_status,
                            target_ref: TargetRef::from(app.target),
                        }).into_any()
                    }
                    (Some(Err(e)), _) | (_, Some(Err(e))) => view! {
                        <div class="min-h-screen flex items-center justify-center bg-slate-950 px-6 text-sm">
                            <div class="max-w-lg rounded-md border border-rose-400/30 bg-rose-500/10 px-5 py-4 text-rose-100">
                                <div class="text-xs font-semibold uppercase tracking-widest text-rose-200">
                                    "Brain unavailable"
                                </div>
                                <p class="mt-2 text-slate-200">
                                    "The last projection could not be loaded. Refresh after checking the target connection."
                                </p>
                                <p class="mt-3 font-mono text-xs text-rose-200/90 break-words">{format!("{e}")}</p>
                            </div>
                        </div>
                    }.into_any(),
                    _ => knowledge_loading_view().into_any(),
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
    config_diagnostic: Option<ConfigLoadDiagnostic>,
    graph_version: RwSignal<u64>,
    sync_status: RwSignal<SyncStatus>,
    target_ref: TargetRef,
) -> impl IntoView {
    provide_context(target_ref.clone());
    let query = use_query_map();
    let nodes = StoredValue::new(nodes);
    let repo_files = StoredValue::new(files);
    let config = StoredValue::new(config);
    let config_diagnostic = StoredValue::new(config_diagnostic);
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
    let total_files = repo_files.with_value(Vec::len);
    let visible_type_count = type_counts.values().filter(|count| **count > 0).count();
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
    let active_search_query = RwSignal::new(String::new());
    // Debounced mirror of the search input. The text field writes to
    // `active_search_query` on every keystroke (so the input feels instant
    // and the clear-x toggles immediately); the URL sync and the server-side
    // `Resource` both key off this debounced signal so we don't navigate /
    // refetch per keystroke. Keying the URL Effect on the live value caused
    // a navigation round-trip per character, which restarted the route's
    // blocking graph Resource — the "graph reload while typing" complaint.
    let debounced_search_query = RwSignal::new(String::new());
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

    fn sorted_vec(set: &HashSet<String>) -> Vec<String> {
        let mut v: Vec<String> = set.iter().cloned().collect();
        v.sort();
        v
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
        let next_search_query = params.get_str("q").map(str::to_string).unwrap_or_default();
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
        if next_search_query != active_search_query.get_untracked() {
            active_search_query.set(next_search_query.clone());
        }
        // Inbound nav also seeds the debounced mirror so deep-linked `?q=`
        // doesn't lag the search panel by 200 ms on first paint.
        if next_search_query != debounced_search_query.get_untracked() {
            debounced_search_query.set(next_search_query);
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
    #[cfg(feature = "hydrate")]
    fn replace_url_without_navigation(target: &str) {
        use wasm_bindgen::JsValue;

        if let Some(window) = web_sys::window()
            && let Ok(history) = window.history()
        {
            let _ = history.replace_state_with_url(&JsValue::NULL, "", Some(target));
        }
    }

    Effect::new(move |_| {
        let tags = active_tags.get();
        let types = active_types.get();
        let path = selected_path.get();
        let path_prefix = active_path_prefix.get();
        let orphan_filter = active_orphan_filter.get();
        let search_query = debounced_search_query.get().trim().to_string();
        let search_query = if search_query_ready(&search_query) {
            search_query
        } else {
            String::new()
        };
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
        if !search_query.is_empty() {
            parts.push(format!("q={}", url_encode(&search_query)));
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
        let current_search_query = current
            .get_str("q")
            .map(|raw| raw.trim().to_string())
            .unwrap_or_default();
        let non_search_params_match = current_tags == tags
            && current_types == types
            && current_path == path
            && current_path_prefix == path_prefix
            && current_orphan_filter == orphan_filter;
        if non_search_params_match && current_search_query == search_query {
            return;
        }
        if non_search_params_match {
            #[cfg(feature = "hydrate")]
            {
                replace_url_without_navigation(&target);
                return;
            }
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

    // Mirror `active_search_query` into `debounced_search_query` after
    // the user stops typing. The Timeout handle lives in a StoredValue so
    // each new keystroke drops the previous timer (cancellation by Drop).
    // Empty and one-character queries flush immediately so the UI clears
    // search state without firing server search or graph filtering.
    #[cfg(feature = "hydrate")]
    {
        let debounce_handle: StoredValue<
            Option<gloo_timers::callback::Timeout>,
            leptos::prelude::LocalStorage,
        > = StoredValue::new_local(None);
        Effect::new(move |_| {
            let next = active_search_query.get();
            if !search_query_ready(&next) {
                debounce_handle.set_value(None);
                if debounced_search_query.get_untracked() != next {
                    debounced_search_query.set(next);
                }
                return;
            }
            let handle = gloo_timers::callback::Timeout::new(SEARCH_DEBOUNCE_MS, move || {
                if debounced_search_query.get_untracked() != next {
                    debounced_search_query.set(next.clone());
                }
            });
            debounce_handle.set_value(Some(handle));
        });
    }
    #[cfg(not(feature = "hydrate"))]
    {
        Effect::new(move |_| {
            let next = active_search_query.get();
            if debounced_search_query.get_untracked() != next {
                debounced_search_query.set(next);
            }
        });
    }

    let target_for_search = target_ref.clone();
    let search_results = Resource::new(
        move || {
            let q = debounced_search_query.get().trim().to_string();
            let q = if search_query_ready(&q) {
                q
            } else {
                String::new()
            };
            (
                q,
                sorted_vec(&active_tags.get()),
                sorted_vec(&active_types.get()),
                active_path_prefix.get(),
            )
        },
        move |(q, tags, node_types, path_prefix)| {
            let target = target_for_search.clone();
            async move {
                if q.trim().is_empty() {
                    return Ok::<Vec<SearchHit>, crate::api::ApiError>(Vec::new());
                }
                search_brain(
                    target,
                    SearchBrainQuery {
                        q,
                        node_types,
                        tags,
                        path_prefix,
                        limit: Some(30),
                    },
                )
                .await
            }
        },
    );

    let search_paths = Memo::new(move |_| {
        if !debounced_search_query.with(|q| search_query_ready(q)) {
            return None;
        }
        search_results
            .get()
            .and_then(Result::ok)
            .map(|hits| hits.into_iter().map(|hit| hit.path).collect::<HashSet<_>>())
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
        let search_paths = search_paths.get();
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
                .filter(|n| {
                    search_paths
                        .as_ref()
                        .is_none_or(|paths| paths.contains(&n.path))
                })
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
            <header class="px-6 py-4 border-b border-slate-800 bg-slate-950/95 flex items-center gap-4">
                <div class="flex min-w-0 items-center gap-3">
                    <div class="flex h-9 w-9 shrink-0 items-center justify-center rounded-md border border-teal-400/30 bg-teal-400/10">
                        <span class="h-2.5 w-2.5 rounded-full bg-teal-300"></span>
                    </div>
                    <div class="min-w-0">
                        <div class="flex items-center gap-2">
                            <h1 class="text-sm font-semibold uppercase tracking-wide text-slate-200">
                                "Knowledge"
                            </h1>
                            <span class="rounded-full border border-emerald-400/30 bg-emerald-500/10 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-widest text-emerald-200">
                                "Live"
                            </span>
                        </div>
                        {(!target_label.is_empty()).then(|| view! {
                            <div class="mt-0.5 truncate font-mono text-xs text-slate-500">{target_label.clone()}</div>
                        })}
                    </div>
                </div>
                <a
                    href=admin_href
                    rel="external"
                    class="rounded-md border border-slate-800 px-2.5 py-1 text-xs text-slate-400 hover:border-slate-700 hover:text-slate-200"
                    title="Open projection, sync, sessions, and audit status"
                >
                    "Status"
                </a>
                <div class="min-w-[240px] max-w-xl flex-1">
                    <label class="relative block">
                        <span class="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-xs text-slate-500">
                            "Search"
                        </span>
                        <input
                            type="text"
                            autocomplete="off"
                            placeholder="Find text in this Brain"
                            class="w-full rounded-md border border-slate-800 bg-slate-900/70 py-2 pl-14 pr-9 text-sm text-slate-100 placeholder:text-slate-600 focus:border-teal-400 focus:outline-none focus:ring-1 focus:ring-teal-500"
                            prop:value=move || active_search_query.get()
                            on:input=move |ev| active_search_query.set(event_target_value(&ev))
                        />
                        <Show when=move || !active_search_query.with(|q| q.trim().is_empty())>
                            <button
                                type="button"
                                class="absolute right-2 top-1/2 -translate-y-1/2 rounded-md px-2 py-1 text-xs text-slate-500 hover:bg-slate-800 hover:text-slate-200"
                                title="Clear search"
                                aria-label="Clear search"
                                on:click=move |_| active_search_query.set(String::new())
                            >
                                <span aria-hidden="true">"x"</span>
                            </button>
                        </Show>
                    </label>
                </div>
                <div class="ml-auto flex items-center gap-2 flex-wrap justify-end">
                    <span
                        class="px-2.5 py-1 rounded-md bg-slate-900/80 border border-slate-800 text-[10px] uppercase tracking-widest text-slate-400"
                        title="Total nodes in the current graph"
                    >
                        <span class="text-slate-200 font-semibold tabular-nums">{total_nodes}</span>
                        " nodes"
                    </span>
                    <span
                        class="px-2.5 py-1 rounded-md bg-slate-900/80 border border-slate-800 text-[10px] uppercase tracking-widest text-slate-400"
                        title="Markdown files indexed in this Brain"
                    >
                        <span class="text-slate-200 font-semibold tabular-nums">{total_files}</span>
                        " files"
                    </span>
                    <span
                        class="px-2.5 py-1 rounded-md bg-slate-900/80 border border-slate-800 text-[10px] uppercase tracking-widest text-slate-400"
                        title="Node types currently represented"
                    >
                        <span class="text-slate-200 font-semibold tabular-nums">{visible_type_count}</span>
                        " types"
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
                                <details class="relative">
                                    <summary
                                        class="list-none px-2.5 py-1 rounded-md bg-slate-900/80 border border-slate-800 text-[10px] uppercase tracking-widest text-slate-400 hover:text-slate-200 cursor-pointer"
                                        title="Show remaining types"
                                    >
                                        "+" {extra} " more"
                                    </summary>
                                    <ul
                                        class="absolute right-0 z-10 mt-1 p-2 shadow-lg bg-slate-900 border border-slate-800 rounded-md min-w-[180px] space-y-1"
                                    >
                                        {rest.into_iter().map(move |(t_name, count)| {
                                            let spec = config_rest.lookup(&t_name).unwrap_or(config_rest.default_spec());
                                            view! {
                                                <li class="px-2 py-1 rounded hover:bg-slate-800">
                                                    <span class="flex items-center justify-between gap-3 text-[11px] text-slate-300">
                                                        <span>{spec.label.clone()}</span>
                                                        <span class="tabular-nums text-slate-400">{count}</span>
                                                    </span>
                                                </li>
                                            }
                                        }).collect_view()}
                                    </ul>
                                </details>
                            }
                        })
                    }
                    <RefreshButton graph_version=graph_version sync_status=sync_status />
                    <button
                        class="ml-2 px-2 py-1 rounded text-[10px] uppercase tracking-widest border border-teal-400/60 text-teal-200 hover:bg-teal-500/10 transition-colors focus:outline-none focus:ring-1 focus:ring-teal-500"
                        title="Create a new Brain document"
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
            <OrphanBanner nodes=nodes config=config diagnostic=config_diagnostic />
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
                <div class="flex-1 relative min-w-0 flex">
                    <GraphCanvas
                        nodes=nodes
                        edges=edges
                        visible_ids=visible_ids.into()
                        hovered=hovered
                        selected=selected
                        selected_path=selected_path
                        config=config.get_value()
                    />
                    <SearchResultsPanel
                        query=active_search_query
                        searched_query=debounced_search_query
                        results=search_results
                        selected_path=selected_path
                    />
                </div>
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

#[component]
fn SearchResultsPanel(
    query: RwSignal<String>,
    searched_query: RwSignal<String>,
    results: Resource<Result<Vec<SearchHit>, crate::api::ApiError>>,
    selected_path: RwSignal<Option<String>>,
) -> impl IntoView {
    view! {
        <Show when=move || {
            let live = query.with(|q| q.trim().to_string());
            let searched = searched_query.with(|q| q.trim().to_string());
            search_query_ready(&live) && live == searched
        }>
            <section class="absolute left-4 top-4 z-20 w-[min(520px,calc(100%-2rem))] rounded-md border border-slate-800 bg-slate-950/95 shadow-2xl shadow-black/30 backdrop-blur">
                <div class="flex items-center justify-between gap-3 border-b border-slate-800 px-3 py-2">
                    <div class="min-w-0">
                        <h2 class="text-[10px] font-semibold uppercase tracking-widest text-slate-500">
                            "Search Results"
                        </h2>
                        <p class="mt-0.5 truncate text-xs text-slate-400">{move || searched_query.get()}</p>
                    </div>
                </div>
                <div class="max-h-[42vh] overflow-y-auto p-2">
                    {move || match results.get() {
                        None => view! {
                            <p class="px-2 py-3 text-xs text-slate-500">"Searching..."</p>
                        }.into_any(),
                        Some(Err(error)) => view! {
                            <p class="px-2 py-3 text-xs text-rose-200">{error.actionable_message()}</p>
                        }.into_any(),
                        Some(Ok(hits)) if hits.is_empty() => view! {
                            <p class="px-2 py-3 text-xs text-slate-500">"No matching nodes."</p>
                        }.into_any(),
                        Some(Ok(hits)) => view! {
                            <div class="space-y-1">
                                {hits.into_iter().map(|hit| {
                                    let path = hit.path.clone();
                                    view! {
                                        <button
                                            type="button"
                                            class="block w-full rounded-md border border-transparent px-3 py-2 text-left hover:border-slate-700 hover:bg-slate-900/80 focus:border-teal-400 focus:outline-none"
                                            on:click=move |_| selected_path.set(Some(path.clone()))
                                        >
                                            <div class="flex items-start justify-between gap-3">
                                                <span class="min-w-0 truncate text-sm font-medium text-slate-100">{hit.title}</span>
                                                <span class="shrink-0 font-mono text-[10px] text-slate-600">{format!("{:.4}", hit.score)}</span>
                                            </div>
                                            <div class="mt-0.5 truncate font-mono text-[10px] text-slate-500">{hit.path}</div>
                                            <p class="mt-1 line-clamp-2 text-xs leading-5 text-slate-300">{hit.snippet}</p>
                                        </button>
                                    }
                                }).collect_view()}
                            </div>
                        }.into_any(),
                    }}
                </div>
            </section>
        </Show>
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
            class="px-2 py-1 rounded text-[10px] uppercase tracking-widest text-slate-400 hover:text-slate-200 hover:bg-slate-800 transition-colors focus:outline-none focus:ring-1 focus:ring-slate-500 disabled:opacity-40 disabled:cursor-not-allowed"
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
                            sync_status.set(SyncStatus::Degraded {
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
