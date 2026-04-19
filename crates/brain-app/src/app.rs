use leptos::prelude::*;
use leptos_meta::{MetaTags, Stylesheet, Title, provide_meta_context};
use leptos_router::{
    StaticSegment,
    components::{Route, Router, Routes},
};

use crate::admin::AdminPage;
use crate::api::get_app_config;
use crate::knowledge::KnowledgePage;
use crate::landing::Landing;

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
    // (landing, detail panel, etc.) can read `brand.name`, `target.blob_base()`
    // etc. without its own server-fn round-trip.
    let app_config = Resource::new(|| (), |_| async move { get_app_config().await });
    provide_context(app_config);

    view! {
        <Stylesheet id="leptos" href="/pkg/brain_ui.css"/>
        <Title text="Brain"/>

        <Router>
            <Routes fallback=|| "Page not found.".into_view()>
                <Route path=StaticSegment("") view=Landing/>
                <Route path=StaticSegment("knowledge") view=KnowledgePage/>
                <Route path=StaticSegment("admin") view=AdminPage/>
            </Routes>
        </Router>
    }
}
