use leptos::prelude::*;
use leptos_meta::{MetaTags, Stylesheet, Title, provide_meta_context};
use leptos_router::{
    ParamSegment, StaticSegment,
    components::{Route, Router, Routes},
};

use crate::admin::{AdminPage, ViewsAdminPage};
use crate::api::get_app_config;
use crate::knowledge::KnowledgePage;
use crate::knowledge::brain_switcher::KnowledgePageForTarget;
use crate::knowledge::live_sync::{LiveSync, SyncStatus, SyncStatusBanner};
use crate::landing::Landing;

/// Wrapper around `RwSignal<u64>` so context lookup is unambiguous — without
/// it, any `RwSignal<u64>` provided elsewhere in the tree would collide.
#[derive(Clone, Copy)]
pub struct GraphVersion(pub RwSignal<u64>);

#[derive(Clone, Copy)]
pub struct SyncStatusSignal(pub RwSignal<SyncStatus>);

/// The shell rendered on the server for every page.
pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en" data-theme="brain">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <meta name="color-scheme" content="dark"/>
                <AutoReload options=options.clone()/>
                <HydrationScripts options/>
                <MetaTags/>
            </head>
            <body class="bg-slate-950">
                <App/>
            </body>
        </html>
    }
}

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    // App config is loaded once and shared via context so any component
    // (landing, detail panel, etc.) can read `brand.name`, `target` (wrapped in
    // `GithubClient` for URL building) etc. without its own server-fn round-trip.
    let app_config = Resource::new(|| (), |_| async move { get_app_config().await });
    provide_context(app_config);

    // Global sync state. `LiveSync` (mounted below) keeps these in sync with
    // server-sent events; the banner reads `sync_status` so admins on settings
    // or work-item pages see staleness, not just /knowledge.
    let graph_version = RwSignal::new(0u64);
    let sync_status = RwSignal::new(SyncStatus::Fresh);
    provide_context(GraphVersion(graph_version));
    provide_context(SyncStatusSignal(sync_status));

    view! {
        <Stylesheet id="leptos" href="/pkg/brain_ui.css"/>
        <Title text="Brain"/>

        <LiveSync graph_version=graph_version sync_status=sync_status />
        <SyncStatusBanner sync_status=sync_status />

        <Router>
            <Routes fallback=|| "Page not found.".into_view()>
                <Route path=StaticSegment("") view=Landing/>
                // Legacy single-target routes (compat layer, still backed by boot env).
                <Route path=StaticSegment("knowledge") view=KnowledgePage/>
                <Route path=StaticSegment("admin") view=AdminPage/>
                <Route
                    path=(StaticSegment("admin"), StaticSegment("views"))
                    view=ViewsAdminPage
                />
                // Multi-tenant 3-segment legacy compat: `/{org}/{repo}/knowledge`
                // and `/{org}/{repo}/admin[/views]`. Branch is resolved sticky
                // via `target_registry` and the page reads the env-default
                // until the redirect-to-canonical wrapper lands in the next
                // commit.
                <Route
                    path=(ParamSegment("org"), ParamSegment("repo"), StaticSegment("knowledge"))
                    view=KnowledgePageForTarget
                />
                <Route
                    path=(ParamSegment("org"), ParamSegment("repo"), StaticSegment("admin"))
                    view=AdminPage
                />
                <Route
                    path=(
                        ParamSegment("org"),
                        ParamSegment("repo"),
                        StaticSegment("admin"),
                        StaticSegment("views"),
                    )
                    view=ViewsAdminPage
                />
                // Canonical 4-segment multi-tenant routes
                // (`/{org}/{repo}/{branch}/...`). Branch is part of the
                // identity now — the same components handle both shapes,
                // reading the branch param when present.
                <Route
                    path=(
                        ParamSegment("org"),
                        ParamSegment("repo"),
                        ParamSegment("branch"),
                        StaticSegment("knowledge"),
                    )
                    view=KnowledgePageForTarget
                />
                <Route
                    path=(
                        ParamSegment("org"),
                        ParamSegment("repo"),
                        ParamSegment("branch"),
                        StaticSegment("admin"),
                    )
                    view=AdminPage
                />
                <Route
                    path=(
                        ParamSegment("org"),
                        ParamSegment("repo"),
                        ParamSegment("branch"),
                        StaticSegment("admin"),
                        StaticSegment("views"),
                    )
                    view=ViewsAdminPage
                />
            </Routes>
        </Router>
    }
}
