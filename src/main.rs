#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    use axum::Router;
    use brain_ui::app::*;
    use brain_ui::server::auth;
    use leptos::logging::log;
    use leptos::prelude::*;
    use leptos_axum::{LeptosRoutes, generate_route_list};
    use tower_sessions::{MemoryStore, SessionManagerLayer};

    dotenvy::dotenv().ok();

    let session_store = MemoryStore::default();
    let session_layer = SessionManagerLayer::new(session_store);

    let conf = get_configuration(None).unwrap();
    let addr = conf.leptos_options.site_addr;
    let leptos_options = conf.leptos_options;
    let routes = generate_route_list(App);

    let app = Router::new()
        .route("/auth/callback", axum::routing::get(auth::oauth_callback))
        .leptos_routes(&leptos_options, routes, {
            let leptos_options = leptos_options.clone();
            move || shell(leptos_options.clone())
        })
        .fallback(leptos_axum::file_and_error_handler(shell))
        .layer(session_layer)
        .with_state(leptos_options);

    log!("listening on http://{}", &addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}

#[cfg(not(feature = "ssr"))]
pub fn main() {
    // no client-side main function
    // see lib.rs for hydration function instead
}
