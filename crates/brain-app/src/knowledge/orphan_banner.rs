//! Advisory banner listing nodes whose `type:` is not declared in
//! `.brain-config.yml`. The lookup fallback already coerces unknown types into
//! `default_spec()` for rendering — this banner makes the silent coercion
//! visible so a typo in frontmatter can't go unnoticed.

use std::collections::BTreeMap;

use leptos::prelude::*;

use super::types::Node;
use crate::api::{AppConfig, ConfigLoadDiagnostic};

#[component]
pub fn OrphanBanner(
    nodes: StoredValue<Vec<Node>>,
    config: StoredValue<brain_domain::BrainConfig>,
    diagnostic: StoredValue<Option<ConfigLoadDiagnostic>>,
) -> impl IntoView {
    let dismissed = RwSignal::new(false);
    let app_config = use_context::<Resource<Result<AppConfig, ServerFnError>>>();
    let target_ref = use_context::<brain_domain::TargetRef>();

    let config_url = Memo::new(move |_| {
        if let Some(target_ref) = target_ref.clone() {
            return brain_domain::GithubClient::new((&target_ref).into()).config_blob_url();
        }
        app_config
            .and_then(|r| r.get())
            .and_then(|r| r.ok())
            .map(|c| brain_domain::GithubClient::new(c.target).config_blob_url())
            .unwrap_or_default()
    });

    if let Some(diagnostic) = diagnostic.get_value() {
        let message = StoredValue::new(diagnostic.message);
        return view! {
            <Show when=move || !dismissed.get()>
                <div class="px-6 py-2 bg-rose-500/10 border-b border-rose-400/40 text-rose-100 text-xs flex items-center gap-3">
                    <span class="font-medium shrink-0">"Config invalid:"</span>
                    <span class="text-rose-200/90 truncate" title=move || message.get_value()>
                        {move || message.get_value()}
                    </span>
                    <a
                        rel="noopener noreferrer"
                        href=move || config_url.get()
                        target="_blank"
                        class="ml-auto shrink-0 px-2 py-0.5 rounded border border-rose-400/50 hover:bg-rose-400/10 transition-colors"
                    >
                        "Open .brain-config.yml →"
                    </a>
                    <button
                        class="px-2 py-0.5 text-rose-300/70 hover:text-rose-100"
                        on:click=move |_| dismissed.set(true)
                        aria-label="Dismiss"
                    >
                        "✕"
                    </button>
                </div>
            </Show>
        }
        .into_any();
    }

    // Group: unknown_type -> list of node titles.
    let orphans: Vec<(String, Vec<String>)> = {
        let known: std::collections::HashSet<String> =
            config.with_value(|c| c.node_types.iter().map(|s| s.name.clone()).collect());
        let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
        nodes.with_value(|ns| {
            for n in ns {
                if !known.contains(&n.node_type) {
                    grouped
                        .entry(n.node_type.clone())
                        .or_default()
                        .push(n.title.clone());
                }
            }
        });
        grouped.into_iter().collect()
    };

    if orphans.is_empty() {
        return ().into_any();
    }

    let total: usize = orphans.iter().map(|(_, v)| v.len()).sum();
    let summary = orphans
        .iter()
        .map(|(t, v)| format!("`{}` ({})", t, v.len()))
        .collect::<Vec<_>>()
        .join(", ");

    view! {
        <Show when=move || !dismissed.get()>
            <div class="px-6 py-2 bg-amber-500/10 border-b border-amber-400/40 text-amber-100 text-xs flex items-center gap-3">
                <span class="font-medium">
                    {format!("{total} node{plural} use types not in .brain-config.yml:",
                             plural = if total == 1 { "" } else { "s" })}
                </span>
                <span class="text-amber-200/90">{summary.clone()}</span>
                <a
                    rel="noopener noreferrer"
                    href=move || config_url.get()
                    target="_blank"
                    class="ml-auto px-2 py-0.5 rounded border border-amber-400/50 hover:bg-amber-400/10 transition-colors"
                >
                    "Add to .brain-config.yml →"
                </a>
                <button
                    class="px-2 py-0.5 text-amber-300/70 hover:text-amber-100"
                    on:click=move |_| dismissed.set(true)
                    aria-label="Dismiss"
                >
                    "✕"
                </button>
            </div>
        </Show>
    }
    .into_any()
}
