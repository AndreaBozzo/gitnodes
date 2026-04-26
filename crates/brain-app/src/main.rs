#![recursion_limit = "512"]
#![cfg_attr(not(test), warn(clippy::unwrap_used))]

#[cfg(feature = "ssr")]
fn required_env_with_legacy(primary: &str, legacy: &str) -> String {
    std::env::var(primary)
        .or_else(|_| std::env::var(legacy))
        .unwrap_or_else(|_| panic!("{primary} or legacy {legacy} must be set"))
}

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
    use brain_app::app::*;
    use brain_app::server::auth;
    use brain_domain::{BrandConfig, TargetConfig};
    use leptos::prelude::*;
    use leptos_axum::{LeptosRoutes, generate_route_list};
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;
    use tower_sessions::{Session, SessionManagerLayer, cookie::SameSite};
    use tower_sessions_sqlx_store::SqliteStore;

    // Explicitly register server functions to ensure the linker doesn't strip them
    // and they are available at runtime.
    use brain_app::api;
    api::register_server_functions();

    dotenvy::dotenv().ok();

    // Runtime config from env — fail fast if any are missing so a misconfigured
    // deploy can't silently write to the wrong repo.
    let target_cfg = TargetConfig {
        org: required_env_with_legacy("TARGET_GITHUB_ORG", "GITHUB_ORG"),
        repo: required_env_with_legacy("TARGET_GITHUB_REPO", "GITHUB_REPO"),
        branch: required_env_with_legacy("TARGET_GITHUB_BRANCH", "GITHUB_BRANCH"),
    };
    let brand_cfg = BrandConfig {
        name: std::env::var("BRAND_NAME").expect("BRAND_NAME must be set"),
        org_label: std::env::var("BRAND_ORG_LABEL").expect("BRAND_ORG_LABEL must be set"),
    };

    // Structured logging. Level controlled by RUST_LOG (defaults to info for our
    // crate, warn elsewhere). Audit log stays as the domain-event stream; this is
    // for operational visibility.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "brain_app=info,warn".into()),
        )
        .init();

    // Single pooled, **target-agnostic** HTTP client for the whole process.
    // Threaded through Leptos context so server fns and the asset proxy share
    // connection state. The transport carries no target binding; each call
    // site supplies the right `TargetConfig` per request — that's what keeps
    // a future Brain-Switcher (Phase 3) from silently reading the wrong repo.
    let gh_http =
        brain_storage::GithubHttp::new().expect("failed to build pooled GitHub HTTP client");
    tracing::info!("github http client built (pooled, target-agnostic)");

    // Persistent runtime store backed by SQLite.
    // Holds sessions, audit events, and the local graph projection.
    // Use a standard URL format. Default to local sqlite.
    let db_url =
        std::env::var("SESSION_DB_URL").unwrap_or_else(|_| "sqlite://data/sessions.db".to_string());

    // Only attempt to create parent directories if it's a local SQLite file
    if db_url.starts_with("sqlite://") && !db_url.starts_with("sqlite://:memory:") {
        let file_path = db_url
            .strip_prefix("sqlite://")
            .expect("db_url starts_with sqlite:// guard above guarantees this prefix");
        if let Some(parent) = std::path::Path::new(file_path).parent()
            && !parent.as_os_str().is_empty()
        {
            // Do not swallow the error! Fail fast if permissions are wrong.
            std::fs::create_dir_all(parent).unwrap_or_else(|e| {
                panic!(
                    "Failed to create session DB directory at {:?}: {}",
                    parent, e
                )
            });
        }
    }

    let sqlite_opts = SqliteConnectOptions::from_str(&db_url)
        .expect("Valid database connection string")
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(sqlite_opts)
        .await
        .expect("failed to open sessions SQLite pool");
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .expect("enable sqlite foreign keys");
    sqlx::query("PRAGMA journal_mode = WAL")
        .execute(&pool)
        .await
        .expect("enable sqlite WAL mode");
    let session_store = SqliteStore::new(pool.clone());
    session_store
        .migrate()
        .await
        .expect("session store migration");
    brain_app::server::audit::migrate(&pool)
        .await
        .expect("audit table migration");
    brain_app::server::projection::migrate(&pool)
        .await
        .expect("projection table migration");
    brain_app::server::audit::init(pool.clone());
    brain_app::server::projection::init(pool.clone());

    let event_bus = brain_app::server::sse::EventBus::new();

    let webhook_secret = std::env::var("WEBHOOK_SECRET").ok();
    if webhook_secret.is_none() {
        tracing::warn!("WEBHOOK_SECRET not set — webhook endpoint will accept unsigned payloads");
    }
    let webhook_state = brain_app::server::webhook::WebhookState {
        bus: event_bus.clone(),
        target: target_cfg.clone(),
        http: gh_http.clone(),
        secret: webhook_secret,
    };
    // OAuth callback is a cross-site redirect back from github.com, so the session
    // cookie must be SameSite=Lax (Strict would drop it and kill CSRF state check).
    // Secure=false allows http://127.0.0.1 in dev; set SESSION_COOKIE_SECURE=1 in prod.
    let cookie_secure = std::env::var("SESSION_COOKIE_SECURE")
        .map(|v| v != "0" && !v.is_empty())
        .unwrap_or(false);
    let session_layer = SessionManagerLayer::new(session_store)
        .with_same_site(SameSite::Lax)
        .with_secure(cookie_secure);

    let conf = get_configuration(None).expect("load Leptos configuration");
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
    // SSE is also gated — without it, anyone can subscribe to `/sse/events` and
    // infer private repo activity (push timing, rebuild failures) from the
    // typed event names. SSE gets `401` instead of a redirect because
    // `EventSource` would otherwise treat the redirect as success and
    // reconnect-loop forever.
    async fn protect_knowledge(session: Session, request: Request<Body>, next: Next) -> Response {
        let path = request.uri().path();
        let needs_auth = path == "/knowledge"
            || path.starts_with("/knowledge/")
            || path == "/admin"
            || path.starts_with("/admin/")
            || path.starts_with("/assets/")
            || path.starts_with("/sse/");
        if needs_auth && !auth::is_authenticated(&session).await {
            if path.starts_with("/sse/") {
                axum::http::StatusCode::UNAUTHORIZED.into_response()
            } else {
                Redirect::to("/").into_response()
            }
        } else {
            next.run(request).await
        }
    }

    let options_for_ssr = leptos_options.clone();

    // Private-repo asset proxy. Raw GitHub URLs would require the user's OAuth
    // token on `<img>` requests, which the browser can't attach — so we serve
    // bytes ourselves via the Contents API with the session's token.
    let asset_router = Router::new()
        .route(
            "/{*path}",
            axum::routing::get(brain_app::server::assets::serve_asset),
        )
        .with_state(brain_app::server::assets::AssetProxyState {
            http: gh_http.clone(),
            target: target_cfg.clone(),
        });

    let app = Router::new()
        .nest("/assets", asset_router)
        .route("/auth/login", axum::routing::get(auth::login))
        .route("/auth/logout", axum::routing::get(auth::logout))
        .route("/auth/callback", axum::routing::get(auth::oauth_callback))
        .route(
            "/sse/events",
            axum::routing::get(brain_app::server::sse::handle).with_state(event_bus),
        )
        .route(
            "/webhook/github",
            axum::routing::post(brain_app::server::webhook::handle).with_state(webhook_state),
        )
        // Server functions: extract Session and inject Session + runtime config
        // into Leptos context so use_context::<...>() works inside #[server] fns.
        .route(
            "/api/{*fn_name}",
            axum::routing::post({
                let target_for_api = target_cfg.clone();
                let brand_for_api = brand_cfg.clone();
                let http_for_api = gh_http.clone();
                move |session: Session, request: Request<Body>| {
                    let target = target_for_api.clone();
                    let brand = brand_for_api.clone();
                    let http = http_for_api.clone();
                    async move {
                        leptos_axum::handle_server_fns_with_context(
                            move || {
                                provide_context(session.clone());
                                provide_context(target.clone());
                                provide_context(brand.clone());
                                provide_context(http.clone());
                            },
                            request,
                        )
                        .await
                    }
                }
            }),
        )
        // SSR page routes: inject Session + configs for any server fns called
        // during SSR.
        .leptos_routes_with_handler(routes, {
            let target_for_ssr = target_cfg.clone();
            let brand_for_ssr = brand_cfg.clone();
            let http_for_ssr = gh_http.clone();
            move |session: Session, request: Request<Body>| {
                let options = options_for_ssr.clone();
                let target = target_for_ssr.clone();
                let brand = brand_for_ssr.clone();
                let http = http_for_ssr.clone();
                async move {
                    let handler = leptos_axum::render_app_to_stream_with_context(
                        move || {
                            provide_context(session.clone());
                            provide_context(target.clone());
                            provide_context(brand.clone());
                            provide_context(http.clone());
                        },
                        move || shell(options.clone()),
                    );
                    handler(request).await
                }
            }
        })
        .fallback(leptos_axum::file_and_error_handler(shell))
        .layer(middleware::from_fn(protect_knowledge))
        .layer(session_layer)
        .with_state(leptos_options);

    tracing::info!(%addr, "brain_app listening");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("bind TCP listener on {addr}: {e}"));
    axum::serve(listener, app.into_make_service())
        .await
        .expect("axum serve loop terminated with error");
}

#[cfg(not(feature = "ssr"))]
pub fn main() {
    // no client-side main function
    // see lib.rs for hydration function instead
}
