use leptos::prelude::*;

/// Read-only tag badge shown in detail panels and the detail bar.
#[component]
pub fn TagBadge(tag: String) -> impl IntoView {
    view! {
        <span class="px-2 py-0.5 rounded text-[10px] bg-slate-800 text-slate-300 border border-slate-700">
            {"#"}{tag}
        </span>
    }
}

/// Removable tag pill used in the editor for tags and related-link chips.
#[component]
pub fn RemovableBadge(
    label: String,
    #[prop(optional)] prefix: &'static str,
    on_remove: impl Fn() + Send + Sync + 'static,
) -> impl IntoView {
    view! {
        <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] bg-teal-400/20 text-teal-200 border border-teal-400/40">
            {prefix}{label}
            <button class="hover:text-red-300" on:click=move |_| on_remove()>
                "×"
            </button>
        </span>
    }
}
