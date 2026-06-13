// Copyright (C) 2026 Andrea Bozzo
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

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
