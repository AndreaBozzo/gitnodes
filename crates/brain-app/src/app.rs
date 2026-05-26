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
        <html lang="en">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <meta name="color-scheme" content="dark"/>
                <AutoReload options=options.clone()/>
                <HydrationScripts options/>
                <MetaTags/>
                <script src="/vendor/mermaid-10.9.3.min.js"></script>
                <script>
                    {r#"
                        window.brainMermaidReady = false;
                        window.brainMermaidObserver = null;
                        window.brainMermaidTimer = null;
                        window.initializeBrainMermaid = function() {
                            if (!window.mermaid || window.brainMermaidReady) {
                                return;
                            }
                            mermaid.initialize({
                                startOnLoad: false,
                                securityLevel: 'strict',
                                theme: 'dark',
                                themeVariables: {
                                    background: '#090d16',
                                    primaryColor: '#1e293b',
                                    primaryTextColor: '#f8fafc',
                                    primaryBorderColor: '#334155',
                                    lineColor: '#64748b',
                                    secondaryColor: '#0f172a',
                                    tertiaryColor: '#1e293b'
                                }
                            });
                            window.brainMermaidReady = true;
                        };
                        window.renderBrainMermaid = function() {
                            clearTimeout(window.brainMermaidTimer);
                            window.brainMermaidTimer = setTimeout(() => {
                                window.initializeBrainMermaid();
                                if (!window.mermaid) {
                                    return;
                                }
                                document.querySelectorAll('pre code.language-mermaid, pre code.mermaid').forEach((el) => {
                                    const pre = el.parentElement;
                                    if (!pre || !pre.parentElement) {
                                        return;
                                    }
                                    const div = document.createElement('div');
                                    div.className = 'mermaid';
                                    div.title = 'Open diagram preview';
                                    div.style.cursor = 'zoom-in';
                                    div.textContent = el.textContent;
                                    pre.parentElement.replaceChild(div, pre);
                                });
                                document.querySelectorAll('.mermaid').forEach((el) => {
                                    el.title = 'Open diagram preview';
                                    el.style.cursor = 'zoom-in';
                                });
                                const pending = Array.from(
                                    document.querySelectorAll('.mermaid:not([data-processed="true"])')
                                );
                                if (pending.length === 0) {
                                    return;
                                }
                                mermaid.run({ nodes: pending }).catch((err) => {
                                    console.error('Unable to render Mermaid diagram', err);
                                });
                            }, 50);
                        };
                        window.startBrainMermaidObserver = function() {
                            if (window.brainMermaidObserver) {
                                return;
                            }
                            const start = () => {
                                if (!document.body || window.brainMermaidObserver) {
                                    return;
                                }
                                window.brainMermaidObserver = new MutationObserver(() => {
                                    window.renderBrainMermaid();
                                });
                                window.brainMermaidObserver.observe(document.body, {
                                    childList: true,
                                    subtree: true
                                });
                                window.renderBrainMermaid();
                            };
                            if (document.body) {
                                start();
                            } else {
                                document.addEventListener('DOMContentLoaded', start, { once: true });
                            }
                        };
                        window.initializeBrainMermaid();
                        window.startBrainMermaidObserver();
                    "#}
                </script>
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
    let app_config = Resource::new_blocking(|| (), |_| async move { get_app_config().await });
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
