use std::collections::{BTreeMap, HashMap, HashSet};

use leptos::prelude::*;

use crate::api::RepoFile;

#[derive(Clone, Debug, Default)]
struct FolderNode {
    name: String,
    path: String,
    children: BTreeMap<String, FolderNode>,
    files: Vec<RepoFile>,
}

#[derive(Clone, Debug, Default)]
struct FolderStats {
    file_count: usize,
    work_item_count: usize,
    orphan_count: usize,
    node_type_counts: HashMap<String, usize>,
}

impl FolderNode {
    fn insert(&mut self, file: RepoFile) {
        let parts: Vec<&str> = file.path.split('/').collect();
        if parts.len() <= 1 {
            self.files.push(file);
            return;
        }

        let mut current = self;
        let mut prefix = String::new();
        for part in &parts[..parts.len() - 1] {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            current = current
                .children
                .entry((*part).to_string())
                .or_insert_with(|| FolderNode {
                    name: (*part).to_string(),
                    path: prefix.clone(),
                    children: BTreeMap::new(),
                    files: Vec::new(),
                });
        }
        current.files.push(file);
    }

    fn stats(&self) -> FolderStats {
        let mut stats = FolderStats::default();
        for file in &self.files {
            stats.file_count += 1;
            if file.is_work_item {
                stats.work_item_count += 1;
            }
            if file.is_orphan_in_graph {
                stats.orphan_count += 1;
            }
            if let Some(node_type) = &file.node_type {
                *stats.node_type_counts.entry(node_type.clone()).or_default() += 1;
            }
        }
        for child in self.children.values() {
            let child_stats = child.stats();
            stats.file_count += child_stats.file_count;
            stats.work_item_count += child_stats.work_item_count;
            stats.orphan_count += child_stats.orphan_count;
            for (node_type, count) in child_stats.node_type_counts {
                *stats.node_type_counts.entry(node_type).or_default() += count;
            }
        }
        stats
    }
}

#[component]
pub fn RepoStructureTree(
    files: Vec<RepoFile>,
    active_path_prefix: RwSignal<Option<String>>,
    active_orphan_filter: RwSignal<bool>,
    selected_path: RwSignal<Option<String>>,
    config: brain_domain::BrainConfig,
    current_org: String,
    current_repo: String,
) -> impl IntoView {
    let config = StoredValue::new(config);
    let root = StoredValue::new(build_tree(files));
    let total = root.with_value(|node| node.stats());
    let storage_key = storage_key(&current_org, &current_repo);
    let expanded = RwSignal::new(HashSet::new());
    let expanded_loaded = RwSignal::new(false);

    #[cfg(feature = "hydrate")]
    Effect::new({
        let storage_key = storage_key.clone();
        move |_| {
            if expanded_loaded.get() {
                return;
            }
            expanded.set(read_expanded(&storage_key));
            expanded_loaded.set(true);
        }
    });

    #[cfg(not(feature = "hydrate"))]
    expanded_loaded.set(true);

    Effect::new({
        let storage_key = storage_key.clone();
        move |_| {
            if !expanded_loaded.get() {
                return;
            }
            write_expanded(&storage_key, &expanded.get())
        }
    });

    let clear_prefix = move |_| active_path_prefix.set(None);
    let active_label = move || {
        active_path_prefix
            .get()
            .map(|prefix| prefix.trim_end_matches('/').to_string())
            .unwrap_or_default()
    };

    view! {
        <section>
            <div class="flex items-center mb-3">
                <h2 class="text-[10px] font-semibold tracking-widest uppercase text-slate-500">
                    "Repository structure"
                </h2>
                <span class="ml-auto text-[10px] text-slate-600">{total.file_count}" files"</span>
            </div>
            <Show when=move || active_path_prefix.with(|p| p.is_some())>
                <div class="mb-2 flex items-center gap-2 rounded border border-teal-400/30 bg-teal-400/10 px-2 py-1 text-[10px] text-teal-100">
                    <span class="min-w-0 flex-1 truncate">"filter: "{active_label}</span>
                    <button class="text-teal-200 hover:text-white" on:click=clear_prefix>"clear"</button>
                </div>
            </Show>
            {(total.orphan_count > 0).then(|| view! {
                <button
                    class="mb-2 w-full rounded border border-amber-400/30 px-2 py-1 text-left text-[10px] text-amber-100 hover:bg-amber-400/15"
                    class=("bg-amber-400/20", move || active_orphan_filter.get())
                    class=("bg-amber-400/10", move || !active_orphan_filter.get())
                    on:click=move |_| active_orphan_filter.update(|value| *value = !*value)
                    title="Files with no wiki links in or out"
                >
                    {total.orphan_count}" isolated markdown files"
                </button>
            })}
            <div class="space-y-0.5 text-[11px]">
                {move || {
                    let root = root.get_value();
                    let rows = root.children.into_values().map(|folder| {
                        render_folder(
                            folder,
                            0,
                            expanded,
                            active_path_prefix,
                            selected_path,
                            config,
                        )
                    }).collect_view();
                    let root_files = root.files.into_iter().map(|file| {
                        render_file(file, 0, selected_path)
                    }).collect_view();
                    view! {
                        <>
                            {rows}
                            {root_files}
                        </>
                    }
                }}
            </div>
        </section>
    }
}

fn build_tree(files: Vec<RepoFile>) -> FolderNode {
    let mut root = FolderNode::default();
    for file in files {
        root.insert(file);
    }
    sort_tree(&mut root);
    root
}

fn sort_tree(node: &mut FolderNode) {
    node.files.sort_by(|a, b| a.path.cmp(&b.path));
    for child in node.children.values_mut() {
        sort_tree(child);
    }
}

fn render_folder(
    folder: FolderNode,
    depth: usize,
    expanded: RwSignal<HashSet<String>>,
    active_path_prefix: RwSignal<Option<String>>,
    selected_path: RwSignal<Option<String>>,
    config: StoredValue<brain_domain::BrainConfig>,
) -> AnyView {
    let path = folder.path.clone();
    let prefix = normalized_prefix(&path);
    let stats = folder.stats();
    let badge = config.with_value(|c| folder_badge(&stats, c));
    let child_dirs = StoredValue::new(folder.children.into_values().collect::<Vec<_>>());
    let files = StoredValue::new(folder.files);
    let is_open_path = path.clone();
    let is_open = Memo::new(move |_| expanded.with(|set| set.contains(&is_open_path)));
    let active_prefix = prefix.clone();
    let is_active = Memo::new(move |_| {
        active_path_prefix.with(|current| current.as_deref() == Some(&active_prefix))
    });
    let indent = format!("padding-left:{}px", depth * 12);

    view! {
        <div>
            <div class="flex items-center gap-1 py-0.5" style=indent>
                <button
                    class="h-4 w-4 shrink-0 rounded text-slate-500 hover:text-slate-200 focus:outline-none focus:ring-1 focus:ring-slate-600"
                    aria-label="Toggle folder"
                    on:click={
                        let path = path.clone();
                        move |_| {
                            expanded.update(|set| {
                                if !set.remove(&path) {
                                    set.insert(path.clone());
                                }
                            });
                        }
                    }
                >
                    {move || if is_open.get() { "v" } else { ">" }}
                </button>
                <button
                    class="min-w-0 flex-1 flex items-center gap-1.5 rounded px-1 py-0.5 text-left transition-colors focus:outline-none focus:ring-1 focus:ring-slate-600"
                    class=("bg-teal-400/15", move || is_active.get())
                    class=("text-teal-100", move || is_active.get())
                    class=("text-slate-300", move || !is_active.get())
                    class=("hover:bg-slate-800/80", move || !is_active.get())
                    title=format!("Filter to {prefix}")
                    on:click={
                        let prefix = prefix.clone();
                        move |_| {
                            active_path_prefix.update(|current| {
                                *current = if current.as_deref() == Some(prefix.as_str()) {
                                    None
                                } else {
                                    Some(prefix.clone())
                                };
                            });
                        }
                    }
                >
                    <span class="truncate">{folder.name}</span>
                    <span class="shrink-0 text-[10px] text-slate-500">{stats.file_count}</span>
                    {(!badge.is_empty()).then(|| view! {
                        <span class="shrink-0 rounded border border-slate-700 px-1 text-[9px] uppercase tracking-wide text-slate-500">{badge}</span>
                    })}
                    {(stats.orphan_count > 0).then(|| view! {
                        <span class="shrink-0 rounded border border-amber-400/40 bg-amber-400/10 px-1 text-[9px] uppercase tracking-wide text-amber-200">{stats.orphan_count}" isolated"</span>
                    })}
                </button>
            </div>
            <Show when=move || is_open.get()>
                {move || child_dirs.get_value().into_iter().map(move |child| {
                    render_folder(
                        child,
                        depth + 1,
                        expanded,
                        active_path_prefix,
                        selected_path,
                        config,
                    )
                }).collect_view()}
                {move || files.get_value().into_iter().map(move |file| render_file(file, depth + 1, selected_path)).collect_view()}
            </Show>
        </div>
    }
    .into_any()
}

fn render_file(file: RepoFile, depth: usize, selected_path: RwSignal<Option<String>>) -> AnyView {
    let path = file.path.clone();
    let label = file
        .path
        .rsplit('/')
        .next()
        .unwrap_or(file.path.as_str())
        .to_string();
    let title = file.title.clone().unwrap_or_else(|| file.path.clone());
    let indent = format!("padding-left:{}px", depth * 12 + 20);
    let is_selected_path = path.clone();
    let is_selected = Memo::new(move |_| {
        selected_path.with(|current| current.as_deref() == Some(&is_selected_path))
    });

    view! {
        <button
            class="flex w-full items-center gap-1 rounded px-1 py-0.5 text-left text-[11px] transition-colors focus:outline-none focus:ring-1 focus:ring-slate-600"
            class=("bg-slate-800", move || is_selected.get())
            class=("text-slate-100", move || is_selected.get())
            class=("text-slate-500", move || !is_selected.get())
            class=("hover:text-slate-200", move || !is_selected.get())
            style=indent
            title=title
            on:click=move |_| selected_path.set(Some(path.clone()))
        >
            <span class="truncate">{label}</span>
            {file.is_work_item.then(|| view! {
                <span class="shrink-0 rounded bg-slate-800 px-1 text-[9px] uppercase tracking-wide text-rose-200">"task"</span>
            })}
            {file.is_orphan_in_graph.then(|| view! {
                <span class="shrink-0 text-amber-200" title="No wiki links in or out">"!"</span>
            })}
        </button>
    }
    .into_any()
}

fn folder_badge(stats: &FolderStats, config: &brain_domain::BrainConfig) -> String {
    if stats.file_count > 0 && stats.file_count == stats.work_item_count {
        return "work items".to_string();
    }
    if stats.node_type_counts.len() == 1
        && let Some((node_type, _)) = stats.node_type_counts.iter().next()
        && let Some(spec) = config.lookup(node_type)
    {
        return spec.label.clone();
    }
    String::new()
}

fn normalized_prefix(prefix: &str) -> String {
    let trimmed = prefix.trim().trim_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}/")
    }
}

fn storage_key(current_org: &str, current_repo: &str) -> String {
    if current_org.is_empty() || current_repo.is_empty() {
        "brain-ui:repo-tree:default".to_string()
    } else {
        format!("brain-ui:repo-tree:{current_org}/{current_repo}")
    }
}

#[cfg(feature = "hydrate")]
fn read_expanded(key: &str) -> HashSet<String> {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item(key).ok().flatten())
        .map(|raw| {
            raw.split('\n')
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(feature = "hydrate")]
fn write_expanded(key: &str, expanded: &HashSet<String>) {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let mut paths: Vec<&str> = expanded.iter().map(String::as_str).collect();
        paths.sort();
        let _ = storage.set_item(key, &paths.join("\n"));
    }
}

#[cfg(not(feature = "hydrate"))]
fn write_expanded(_key: &str, _expanded: &HashSet<String>) {}
