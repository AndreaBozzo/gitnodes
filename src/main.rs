#![recursion_limit = "512"]

#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    use axum::{
        Router,
        body::Body,
        extract::Request,
        middleware::{self, Next},
        response::{IntoResponse, Redirect, Response},
    };
    use brain_ui::app::*;
    use brain_ui::server::auth;
    use leptos::logging::log;
    use leptos::prelude::*;
    use leptos_axum::{LeptosRoutes, generate_route_list};
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;
    use tower_sessions::{Session, SessionManagerLayer};
    use tower_sessions_sqlx_store::SqliteStore;

    dotenvy::dotenv().ok();

    // Persistent session store backed by SQLite.
    // Path is configurable via SESSION_DB_PATH; defaults to ./data/sessions.db.
    let db_path =
        std::env::var("SESSION_DB_PATH").unwrap_or_else(|_| "data/sessions.db".to_string());
    if let Some(parent) = std::path::Path::new(&db_path).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    let sqlite_opts = SqliteConnectOptions::from_str(&format!("sqlite://{db_path}"))
        .expect("valid SESSION_DB_PATH")
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(sqlite_opts)
        .await
        .expect("failed to open sessions SQLite pool");
    let session_store = SqliteStore::new(pool);
    session_store
        .migrate()
        .await
        .expect("session store migration");
    let session_layer = SessionManagerLayer::new(session_store);

    let conf = get_configuration(None).unwrap();
    let addr = conf.leptos_options.site_addr;
    let leptos_options = conf.leptos_options;
    let routes = generate_route_list(App);

    // Path-aware auth gate: blocks anything under `/knowledge` for anonymous users.
    async fn protect_knowledge(session: Session, request: Request<Body>, next: Next) -> Response {
        let path = request.uri().path();
        let needs_auth = path == "/knowledge" || path.starts_with("/knowledge/");
        if needs_auth && !auth::is_authenticated(&session).await {
            Redirect::to("/").into_response()
        } else {
            next.run(request).await
        }
    }

    let app = Router::new()
        .route("/auth/login", axum::routing::get(auth::login))
        .route("/auth/logout", axum::routing::get(auth::logout))
        .route("/auth/callback", axum::routing::get(auth::oauth_callback))
        .leptos_routes(&leptos_options, routes, {
            let leptos_options = leptos_options.clone();
            move || shell(leptos_options.clone())
        })
        .fallback(leptos_axum::file_and_error_handler(shell))
        .layer(middleware::from_fn(protect_knowledge))
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
