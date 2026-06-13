use leptos::prelude::*;

pub(super) fn slugify_title(title: &str) -> String {
    title
        .replace(' ', "-")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect()
}

#[component]
pub(super) fn LocationPicker(
    folder: RwSignal<String>,
    node_type: RwSignal<String>,
    all_folders: Resource<Vec<String>>,
    path_preview: Signal<Option<String>>,
    is_edit: Memo<bool>,
    config: gitnodes_domain::BrainConfig,
) -> impl IntoView {
    let preview_folder = move || {
        path_preview
            .get()
            .and_then(|path| path.rsplit_once('/').map(|(folder, _)| folder.to_string()))
            .unwrap_or_default()
    };
    let new_folder = move || {
        let folder = preview_folder();
        if folder.is_empty() {
            return None;
        }
        let existing = all_folders.get().unwrap_or_default();
        (!existing.iter().any(|f| f.trim_matches('/') == folder)).then_some(folder)
    };

    view! {
        <Show when=move || !is_edit.get()>
            <div>
                <label class="text-[10px] uppercase tracking-widest text-slate-500 mb-1 block">"Location"</label>
                <div class="relative">
                    <input
                        type="text"
                        list="brain-folders"
                        class="w-full px-3 py-2 rounded-md bg-slate-800 border border-slate-700 text-slate-100 text-sm focus:border-teal-400 focus:outline-none"
                        placeholder={
                            let config = config.clone();
                            move || config.lookup(&node_type.get()).map(|s| s.directory.clone()).unwrap_or_default()
                        }
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
                <div class="mt-2 rounded-md border border-slate-800 bg-slate-950/60 px-3 py-2 text-[11px]">
                    <div class="text-slate-500 uppercase tracking-widest text-[9px]">"Will be saved as"</div>
                    <div class="mt-1 font-mono text-slate-200 break-all">
                        {move || path_preview.get().unwrap_or_else(|| "Choose a title to preview the path".to_string())}
                    </div>
                    <Show when=move || new_folder().is_some()>
                        <div class="mt-1 text-amber-200">
                            {move || format!("new folder: {}/", new_folder().unwrap_or_default())}
                        </div>
                    </Show>
                </div>
            </div>
        </Show>
    }
}
