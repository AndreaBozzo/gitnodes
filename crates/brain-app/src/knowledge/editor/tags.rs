use leptos::prelude::*;

use crate::knowledge::components::RemovableBadge;

/// Tag pills, autocomplete input, and suggestion buttons.
#[component]
pub(super) fn TagInput(
    tags: RwSignal<Vec<String>>,
    all_tags: StoredValue<Vec<String>>,
) -> impl IntoView {
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

    let add_tag_token = move |raw: String| {
        let t = normalize_tag(&raw);
        if t.is_empty() {
            return;
        }
        tags.update(|v| {
            if !v.contains(&t) {
                v.push(t);
            }
        });
        tag_input.set(String::new());
    };

    let add_tags_from_input = move |raw: String| {
        for tag in tags_from_input(&raw) {
            add_tag_token(tag);
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
                        add_tags_from_input(tag_input.get_untracked());
                    }
                }
                on:blur=move |_| add_tags_from_input(tag_input.get_untracked())
            />
            <div class="flex flex-wrap gap-1 mt-1">
                {move || tag_suggestions.get().into_iter().map(|t| {
                    let t_click = t.clone();
                    view! {
                        <button
                            type="button"
                            class="px-2 py-0.5 rounded text-[10px] bg-slate-800 text-slate-400 border border-slate-700 hover:text-teal-200 hover:border-teal-400/40 transition-colors focus:outline-none focus:ring-1 focus:ring-slate-500"
                            on:mousedown=move |ev| {
                                ev.prevent_default();
                                add_tag_token(t_click.clone());
                            }
                        >
                            {"+ #"}{t}
                        </button>
                    }
                }).collect_view()}
            </div>
        </div>
    }
}

fn normalize_tag(raw: &str) -> String {
    raw.trim().trim_start_matches('#').trim().to_string()
}

fn tags_from_input(raw: &str) -> Vec<String> {
    raw.split(|c: char| c.is_whitespace() || c == ',')
        .map(normalize_tag)
        .filter(|tag| !tag.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_helpers_split_manual_input_but_preserve_exact_suggestions() {
        assert_eq!(
            tags_from_input(" #brain-ui, rustlang  ops "),
            vec!["brain-ui", "rustlang", "ops"]
        );
        assert_eq!(normalize_tag(" #customer success "), "customer success");
    }
}
