use leptos::prelude::*;
use std::collections::BTreeMap;

pub(super) fn frontmatter_string_fields(
    config: &brain_domain::BrainConfig,
    node_type: &str,
    frontmatter: Option<&BTreeMap<String, serde_yaml::Value>>,
) -> BTreeMap<String, String> {
    let spec = config
        .lookup(node_type)
        .unwrap_or_else(|| config.default_spec());
    let mut out = BTreeMap::new();
    let mut managed = std::collections::BTreeSet::from([
        "type".to_string(),
        "author".to_string(),
        "tags".to_string(),
    ]);
    if let Some(key) = spec.title_key.as_deref() {
        managed.insert(key.to_string());
    }
    if let Some(key) = spec.date_create_field.as_deref() {
        managed.insert(key.to_string());
    }
    if let Some(key) = spec.date_update_field.as_deref() {
        managed.insert(key.to_string());
    }
    if spec.is_work_item() {
        managed.insert("brain_id".to_string());
        managed.insert("state".to_string());
        managed.insert("system_of_record".to_string());
        managed.insert("assignees".to_string());
    }

    for (key, value) in &spec.frontmatter_seed {
        if managed.contains(key) {
            continue;
        }
        if let Some(text) = value.as_str() {
            out.insert(key.clone(), text.to_string());
        }
    }

    if let Some(frontmatter) = frontmatter {
        for (key, value) in frontmatter {
            if managed.contains(key) {
                continue;
            }
            if let Some(text) = value.as_str() {
                out.insert(key.clone(), text.to_string());
            }
        }
    }

    out
}

fn frontmatter_field_label(key: &str) -> String {
    key.split(['_', '-'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Type selector + title + author fields.
#[component]
pub(super) fn FrontmatterFields(
    node_type: RwSignal<String>,
    title: RwSignal<String>,
    author: RwSignal<String>,
    config: brain_domain::BrainConfig,
) -> impl IntoView {
    view! {
        <div>
            <label class="text-[10px] uppercase tracking-widest text-slate-500 mb-1 block">"Type"</label>
            <div class="flex flex-wrap gap-2">
                {config.creatable().map(|spec| {
                    let t = spec.name.clone();
                    let is_active = Memo::new({
                        let t = t.clone();
                        move |_| node_type.get() == t
                    });
                    view! {
                        <button
                            class="px-3 py-1 rounded-full text-xs border transition-colors flex items-center gap-2 focus:outline-none focus:ring-1 focus:ring-slate-500"
                            class=("bg-slate-100", move || is_active.get())
                            class=("text-slate-900", move || is_active.get())
                            class=("border-slate-100", move || is_active.get())
                            class=("text-slate-300", move || !is_active.get())
                            class=("border-slate-700", move || !is_active.get())
                            on:click={
                                let t = t.clone();
                                move |_| node_type.set(t.clone())
                            }
                        >
                            <span class="inline-block w-2 h-2 rounded-full" style=format!("background:{}", spec.accent_var())></span>
                            {spec.label.clone()}
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

/// Operational fields for work item types: state, system of record, assignees.
/// Visible only when the selected node type has `work_item_kind` set.
#[component]
pub(super) fn WorkItemFields(
    wi_state: RwSignal<String>,
    wi_system_of_record: RwSignal<String>,
    wi_assignees: RwSignal<String>,
) -> impl IntoView {
    let select_class = "w-full px-3 py-2 rounded-md bg-slate-800 border border-slate-700 text-slate-100 text-sm focus:border-teal-400 focus:outline-none";
    view! {
        <div class="space-y-3 pt-2 border-t border-slate-800">
            <div>
                <label class="text-[10px] uppercase tracking-widest text-slate-500 mb-1 block">"State"</label>
                <select
                    class=select_class
                    prop:value=move || wi_state.get()
                    on:change=move |ev| wi_state.set(event_target_value(&ev))
                >
                    <option value="backlog">"Backlog"</option>
                    <option value="todo">"Todo"</option>
                    <option value="in-progress">"In Progress"</option>
                    <option value="blocked">"Blocked"</option>
                    <option value="done">"Done"</option>
                    <option value="cancelled">"Cancelled"</option>
                </select>
            </div>

            <div>
                <label class="text-[10px] uppercase tracking-widest text-slate-500 mb-1 block">"System of Record"</label>
                <select
                    class=select_class
                    prop:value=move || wi_system_of_record.get()
                    on:change=move |ev| wi_system_of_record.set(event_target_value(&ev))
                >
                    <option value="brain">"Brain (local only)"</option>
                    <option value="split">"Split (brain + forge)"</option>
                    <option value="external">"External (forge only)"</option>
                </select>
            </div>

            <div>
                <label class="text-[10px] uppercase tracking-widest text-slate-500 mb-1 block">"Assignees"</label>
                <input
                    type="text"
                    class="w-full px-3 py-2 rounded-md bg-slate-800 border border-slate-700 text-slate-100 text-sm focus:border-teal-400 focus:outline-none"
                    placeholder="user1, user2"
                    prop:value=move || wi_assignees.get()
                    on:input=move |ev| wi_assignees.set(event_target_value(&ev))
                />
                <p class="text-[10px] text-slate-600 mt-1">"Comma-separated GitHub usernames."</p>
            </div>
        </div>
    }
}

#[component]
pub(super) fn ExtraFrontmatterFields(fields: RwSignal<BTreeMap<String, String>>) -> impl IntoView {
    view! {
        <Show when=move || !fields.with(BTreeMap::is_empty)>
            <div class="space-y-3 pt-2 border-t border-slate-800">
                <p class="text-[10px] uppercase tracking-widest text-slate-500">"Metadata"</p>
                {move || fields.get().into_keys().map(|key| {
                    let input_key = key.clone();
                    let label = frontmatter_field_label(&key);
                    let placeholder = if key == "status" {
                        "e.g. draft, accepted"
                    } else {
                        ""
                    };
                    view! {
                        <div>
                            <label class="text-[10px] uppercase tracking-widest text-slate-500 mb-1 block">{label}</label>
                            <input
                                type="text"
                                class="w-full px-3 py-2 rounded-md bg-slate-800 border border-slate-700 text-slate-100 text-sm focus:border-teal-400 focus:outline-none"
                                placeholder=placeholder
                                prop:value={
                                    let key = input_key.clone();
                                    move || fields.with(|map| map.get(&key).cloned().unwrap_or_default())
                                }
                                on:input={
                                    let key = input_key.clone();
                                    move |ev| {
                                        let value = event_target_value(&ev);
                                        fields.update(|map| {
                                            map.insert(key.clone(), value.clone());
                                        });
                                    }
                                }
                            />
                        </div>
                    }
                }).collect_view()}
            </div>
        </Show>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_string_fields_seeds_adr_status() {
        let fields = frontmatter_string_fields(&brain_domain::BrainConfig::default(), "adr", None);
        assert_eq!(fields.get("status").map(String::as_str), Some("draft"));
    }

    #[test]
    fn frontmatter_string_fields_ignores_managed_keys() {
        let mut frontmatter = BTreeMap::new();
        frontmatter.insert("author".into(), serde_yaml::Value::String("alice".into()));
        frontmatter.insert(
            "status".into(),
            serde_yaml::Value::String("accepted".into()),
        );
        let fields = frontmatter_string_fields(
            &brain_domain::BrainConfig::default(),
            "adr",
            Some(&frontmatter),
        );
        assert!(!fields.contains_key("author"));
        assert_eq!(fields.get("status").map(String::as_str), Some("accepted"));
    }
}
