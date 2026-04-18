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
    use tower_sessions::{Session, SessionManagerLayer, cookie::SameSite};
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
    // OAuth callback is a cross-site redirect back from github.com, so the session
    // cookie must be SameSite=Lax (Strict would drop it and kill CSRF state check).
    // Secure=false allows http://127.0.0.1 in dev; set SESSION_COOKIE_SECURE=1 in prod.
    let cookie_secure = std::env::var("SESSION_COOKIE_SECURE")
        .map(|v| v != "0" && !v.is_empty())
        .unwrap_or(false);
    let session_layer = SessionManagerLayer::new(session_store)
        .with_same_site(SameSite::Lax)
        .with_secure(cookie_secure);

    let conf = get_configuration(None).unwrap();
    let leptos_options = conf.leptos_options;

    // Railway (and similar PaaS) set $PORT at runtime.
    // Override the address from Cargo.toml when LEPTOS_SITE_ADDR or PORT is set.
    let addr: std::net::SocketAddr = if let Ok(val) = std::env::var("LEPTOS_SITE_ADDR") {
        val.parse().expect("LEPTOS_SITE_ADDR must be host:port")
    } else if let Ok(port) = std::env::var("PORT") {
        format!("0.0.0.0:{port}")
            .parse()
            .expect("PORT must be a valid port number")
    } else {
        leptos_options.site_addr
    };

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

    let options_for_ssr = leptos_options.clone();

    let app = Router::new()
        .route("/auth/login", axum::routing::get(auth::login))
        .route("/auth/logout", axum::routing::get(auth::logout))
        .route("/auth/callback", axum::routing::get(auth::oauth_callback))
        // Server functions: extract Session and inject it into Leptos context
        // so use_context::<Session>() works inside #[server] fns.
        .route(
            "/api/{*fn_name}",
            axum::routing::post(|session: Session, request: Request<Body>| async move {
                leptos_axum::handle_server_fns_with_context(
                    move || provide_context(session.clone()),
                    request,
                )
                .await
            }),
        )
        // SSR page routes: custom handler that also injects Session so
        // server functions called during SSR can access the session.
        .leptos_routes_with_handler(routes, move |session: Session, request: Request<Body>| {
            let options = options_for_ssr.clone();
            async move {
                let handler = leptos_axum::render_app_to_stream_with_context(
                    move || provide_context(session.clone()),
                    move || shell(options.clone()),
                );
                handler(request).await
            }
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
