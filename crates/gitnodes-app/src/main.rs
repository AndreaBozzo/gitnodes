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

#![recursion_limit = "512"]
#![cfg_attr(not(test), warn(clippy::unwrap_used))]

#[cfg(all(feature = "ssr", not(feature = "embed-assets")))]
fn absolute_site_root(
    base: &std::path::Path,
    configured: Option<std::ffi::OsString>,
) -> std::path::PathBuf {
    let root = configured
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("target/site"));
    if root.is_absolute() {
        root
    } else {
        base.join(root)
    }
}

#[cfg(feature = "ssr")]
fn is_protected_path(path: &str) -> bool {
    path == "/knowledge"
        || path.starts_with("/knowledge/")
        || path == "/admin"
        || path.starts_with("/admin/")
        || path == "/assets"
        || path.starts_with("/assets/")
        || path == "/sse"
        || path.starts_with("/sse/")
        // Multi-tenant: /{org}/{repo}/knowledge, /admin, and /assets.
        || is_multi_tenant_protected(path)
}

#[cfg(feature = "ssr")]
fn is_multi_tenant_protected(path: &str) -> bool {
    // Match both 3-segment legacy (`/{org}/{repo}/{knowledge|admin|assets}[/...]`)
    // and 4-segment canonical (`/{org}/{repo}/{branch}/{knowledge|admin}[/...]`).
    // Asset proxy stays 3-segment in alpha; branch-aware asset routing is
    // deferred to beta.
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let legacy_target_route = segments
        .get(2)
        .is_some_and(|segment| matches!(*segment, "knowledge" | "admin" | "assets"));
    let canonical_target_route = segments
        .get(3)
        .is_some_and(|segment| matches!(*segment, "knowledge" | "admin"));
    legacy_target_route || canonical_target_route
}

/// True for the two asset-proxy mounts: `/assets/...` and the multi-tenant
/// `/{org}/{repo}/assets/...`. Used to serve proxied repo bytes under a
/// locked-down CSP — they're untrusted content rendered same-origin, and an
/// SVG opened as a top-level document would otherwise run inline scripts under
/// the permissive page CSP. (A branch literally named `assets` is already
/// shadowed by the asset mount in the router, so this shares that classification.)
#[cfg(feature = "ssr")]
fn is_asset_path(path: &str) -> bool {
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    segments.first() == Some(&"assets") || segments.get(2) == Some(&"assets")
}

/// Decide whether a mutating request is same-origin and may proceed.
///
/// `Origin`, when present, is authoritative: its host must match `Host`. We do
/// NOT let a `Sec-Fetch-Site` header override a present-but-mismatched `Origin`,
/// otherwise a crafted request could set `Sec-Fetch-Site: none` to bypass the
/// check. `Sec-Fetch-Site` is only consulted as a fallback when `Origin` is
/// absent (`same-origin`/`none` trusted, `cross-site`/`same-site` rejected).
/// With neither header the request is allowed: same-origin navigations and
/// non-browser clients (curl, server-to-server) routinely omit both, and the
/// SameSite=Lax session cookie already blocks the cross-site browser case.
#[cfg(feature = "ssr")]
fn is_same_origin(host: Option<&str>, origin: Option<&str>, sec_fetch_site: Option<&str>) -> bool {
    if let Some(origin) = origin {
        // Strip scheme, compare host[:port] against the Host header. A present
        // Origin is always validated, regardless of Sec-Fetch-Site.
        let origin_host = origin
            .split_once("://")
            .map(|(_, rest)| rest)
            .unwrap_or(origin);
        return match host {
            Some(host) => origin_host.eq_ignore_ascii_case(host),
            None => false,
        };
    }
    // No Origin: fall back to Sec-Fetch-Site, else allow (non-browser / nav).
    match sec_fetch_site {
        Some("same-origin") | Some("none") => true,
        Some(_) => false,
        None => true,
    }
}

#[cfg(all(test, feature = "ssr"))]
mod route_protection_tests {
    use super::{absolute_site_root, is_asset_path, is_protected_path, is_same_origin};

    #[test]
    fn asset_path_classification_covers_both_mounts() {
        // Both asset-proxy mounts get the locked-down CSP…
        assert!(is_asset_path("/assets/2026/04/a.png"));
        assert!(is_asset_path("/Dritara-Digital/Brain/assets/2026/04/a.svg"));
        // …while every page/api surface keeps the permissive page CSP.
        assert!(!is_asset_path("/knowledge"));
        assert!(!is_asset_path("/Dritara-Digital/Brain/knowledge"));
        assert!(!is_asset_path("/Dritara-Digital/Brain/main/knowledge"));
        assert!(!is_asset_path("/Dritara-Digital/Brain/main/admin"));
        assert!(!is_asset_path("/api/save_brain_file"));
        assert!(!is_asset_path("/"));
    }

    #[test]
    fn relative_site_root_stays_anchored_to_launch_directory() {
        let launch_dir = std::path::Path::new("/opt/gitnodes");
        assert_eq!(
            absolute_site_root(launch_dir, None),
            launch_dir.join("target/site")
        );
        assert_eq!(
            absolute_site_root(launch_dir, Some("custom/site".into())),
            launch_dir.join("custom/site")
        );
        assert_eq!(
            absolute_site_root(launch_dir, Some("/srv/gitnodes/site".into())),
            std::path::PathBuf::from("/srv/gitnodes/site")
        );
    }

    #[test]
    fn csrf_same_origin_contract() {
        let host = Some("brain.example.com");
        // A present Origin is authoritative and wins over any Sec-Fetch-Site:
        // a mismatched Origin is rejected even with Sec-Fetch-Site: none, and a
        // matching Origin is accepted even if Sec-Fetch-Site says cross-site.
        assert!(!is_same_origin(
            host,
            Some("https://evil.example"),
            Some("none")
        ));
        assert!(is_same_origin(
            host,
            Some("https://brain.example.com"),
            Some("cross-site")
        ));
        // Sec-Fetch-Site only consulted when Origin is absent.
        assert!(is_same_origin(host, None, Some("same-origin")));
        assert!(is_same_origin(host, None, Some("none")));
        assert!(!is_same_origin(host, None, Some("same-site")));
        assert!(!is_same_origin(host, None, Some("cross-site")));
        // Origin host must match Host when Sec-Fetch-Site is absent.
        assert!(is_same_origin(
            host,
            Some("https://brain.example.com"),
            None
        ));
        assert!(is_same_origin(host, Some("http://brain.example.com"), None));
        assert!(!is_same_origin(host, Some("https://attacker.test"), None));
        // Origin with port mismatch is cross-origin.
        assert!(!is_same_origin(
            host,
            Some("https://brain.example.com:8443"),
            None
        ));
        // No Origin and no Sec-Fetch-Site: allowed (non-browser / same-origin nav).
        assert!(is_same_origin(host, None, None));
        // Origin present but no Host header: cannot verify, reject.
        assert!(!is_same_origin(
            None,
            Some("https://brain.example.com"),
            None
        ));
    }

    #[test]
    fn protected_path_contract_covers_workspace_surfaces() {
        let cases = [
            ("/knowledge", true),
            ("/knowledge/node", true),
            ("/admin", true),
            ("/admin/views", true),
            ("/assets", true),
            ("/assets/2026/04/a.png", true),
            ("/sse", true),
            ("/sse/events", true),
            ("/Dritara-Digital/Brain/knowledge", true),
            ("/Dritara-Digital/Brain/knowledge/foo", true),
            ("/Dritara-Digital/Brain/admin", true),
            ("/Dritara-Digital/Brain/admin/views", true),
            ("/Dritara-Digital/Brain/assets/2026/04/a.png", true),
            ("/Dritara-Digital/Brain/main/knowledge", true),
            ("/Dritara-Digital/Brain/main/knowledge/foo", true),
            ("/Dritara-Digital/Brain/main/admin", true),
            ("/", false),
            ("/healthz", false),
            ("/readyz", false),
            ("/auth/login", false),
            ("/auth/callback", false),
            ("/webhook/github", false),
            ("/api/get_current_user", false),
            ("/pkg/gitnodes.js", false),
        ];

        for (path, expected) in cases {
            assert_eq!(
                is_protected_path(path),
                expected,
                "protected route classification changed for {path}"
            );
        }
    }
}

#[cfg(feature = "ssr")]
fn main() {
    // SSR rendering of a fully-populated brain recurses proportionally to the
    // graph/markdown content, which can exceed tokio's default 2 MiB worker
    // stack (seen first via `gitnodes preview`, where the projection is
    // pre-seeded and every request is authenticated). Give the runtime threads
    // a generous stack so a direct hit on a content-heavy page renders instead
    // of aborting with a stack overflow.
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(16 * 1024 * 1024)
        .build()
        .expect("failed to build tokio runtime")
        .block_on(run());
}

#[cfg(feature = "ssr")]
async fn run() {
    use axum::{
        Router,
        body::Body,
        extract::{DefaultBodyLimit, Request},
        http::HeaderValue,
        http::header::{CACHE_CONTROL, PRAGMA},
        middleware::{self, Next},
        response::{IntoResponse, Redirect, Response},
    };
    use gitnodes_app::app::*;
    use gitnodes_app::server::auth;
    use leptos::prelude::*;
    use leptos_axum::{LeptosRoutes, generate_route_list};
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;
    use std::time::Duration;
    use tower_sessions::{Session, SessionManagerLayer, cookie::SameSite};
    use tower_sessions_sqlx_store::SqliteStore;

    #[cfg(not(feature = "embed-assets"))]
    let launch_dir = std::env::current_dir().expect("resolve launch directory");
    #[cfg(not(feature = "embed-assets"))]
    let inherited_site_root = std::env::var_os("LEPTOS_SITE_ROOT");

    // Subcommand dispatch. `serve [dir]` (or no command) runs the server below.
    // The remaining commands exit without starting the web runtime.
    let argv: Vec<String> = std::env::args().collect();
    // `preview [dir]` serves the local working tree read-only with no GitHub.
    let mut local_preview = false;
    let serve_dir = match argv.get(1).map(String::as_str) {
        Some("init") => match gitnodes_app::cli::run_init(argv.get(2).map(String::as_str)) {
            Ok(()) => std::process::exit(0),
            Err(message) => {
                eprintln!("error: {message}");
                std::process::exit(1);
            }
        },
        Some("agents") => match gitnodes_app::cli::run_agents(argv.get(2).map(String::as_str)) {
            Ok(()) => std::process::exit(0),
            Err(message) => {
                eprintln!("error: {message}");
                std::process::exit(1);
            }
        },
        Some("mcp") => {
            if argv.len() > 3 {
                eprintln!("error: `gitnodes mcp` accepts at most one directory\n");
                gitnodes_app::cli::print_usage();
                std::process::exit(2);
            }
            match gitnodes_app::mcp::run(argv.get(2).map(String::as_str)).await {
                Ok(()) => std::process::exit(0),
                Err(message) => {
                    eprintln!("error: {message}");
                    std::process::exit(1);
                }
            }
        }
        Some("doctor") => {
            let mut dir = None;
            let mut json = false;
            for argument in argv.iter().skip(2) {
                if argument == "--json" {
                    json = true;
                } else if argument.starts_with('-') {
                    eprintln!("error: unknown `gitnodes doctor` option {argument:?}");
                    std::process::exit(2);
                } else if dir.replace(argument.as_str()).is_some() {
                    eprintln!("error: `gitnodes doctor` accepts at most one directory");
                    std::process::exit(2);
                }
            }
            match gitnodes_app::cli::run_doctor(dir, json) {
                Ok(true) => std::process::exit(0),
                Ok(false) => std::process::exit(1),
                Err(message) => {
                    eprintln!("error: {message}");
                    std::process::exit(1);
                }
            }
        }
        Some("help") | Some("--help") | Some("-h") => {
            gitnodes_app::cli::print_usage();
            std::process::exit(0);
        }
        Some("version") | Some("--version") | Some("-V") => {
            println!("gitnodes {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }
        Some("serve") => {
            if argv.len() > 3 {
                eprintln!("error: `gitnodes serve` accepts at most one directory\n");
                gitnodes_app::cli::print_usage();
                std::process::exit(2);
            }
            argv.get(2).map(String::as_str)
        }
        Some("preview") => {
            if argv.len() > 3 {
                eprintln!("error: `gitnodes preview` accepts at most one directory\n");
                gitnodes_app::cli::print_usage();
                std::process::exit(2);
            }
            local_preview = true;
            argv.get(2).map(String::as_str)
        }
        None => None,
        Some(other) => {
            eprintln!("error: unknown command '{other}'\n");
            gitnodes_app::cli::print_usage();
            std::process::exit(2);
        }
    };
    if let Err(message) = gitnodes_app::cli::enter_serve_dir(serve_dir) {
        eprintln!("error: {message}");
        std::process::exit(1);
    }

    // Explicitly register server functions to ensure the linker doesn't strip them
    // and they are available at runtime.
    use gitnodes_app::api;
    api::register_server_functions();

    dotenvy::dotenv().ok();
    #[cfg(not(feature = "embed-assets"))]
    {
        let configured_site_root = std::env::var_os("LEPTOS_SITE_ROOT");
        let site_root_base = if inherited_site_root.is_some() || configured_site_root.is_none() {
            launch_dir.clone()
        } else {
            std::env::current_dir().expect("resolve serve directory")
        };
        let site_root = absolute_site_root(&site_root_base, configured_site_root);
        // SAFETY: startup has not spawned application tasks yet. Converting the
        // path to absolute here prevents `serve [dir]` / `preview [dir]` from
        // looking for `target/site` inside the knowledge repository.
        unsafe { std::env::set_var("LEPTOS_SITE_ROOT", &site_root) };
    }
    // Preview mode skips GitHub discovery and `gh auth` entirely; it serves the
    // local working tree only.
    let discovery_notes = if local_preview {
        Vec::new()
    } else {
        match gitnodes_app::cli::configure_local_serve() {
            Ok(notes) => notes,
            Err(message) => {
                eprintln!("error: {message}");
                std::process::exit(1);
            }
        }
    };

    // Structured logging. Level controlled by RUST_LOG (defaults to info for our
    // crate, warn elsewhere). Audit log stays as the domain-event stream; this is
    // for operational visibility.
    //
    // MUST be initialized before any `required_env*` call: those helpers exit(1)
    // via `tracing::error!` on missing env, and without a subscriber installed
    // the diagnostic line vanishes — container logs would show only "exit 1"
    // with no clue which variable was missing.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "gitnodes_app=info,warn".into()),
        )
        .init();
    for note in discovery_notes {
        tracing::info!("{note}");
    }

    // Preview mode synthesizes its target from the working-tree directory and
    // skips the login-org policy; otherwise resolve the GitHub target from env.
    let target_cfg = if local_preview {
        match gitnodes_app::server::local::activate(".") {
            Ok(target) => {
                tracing::info!(
                    repo = %target.repo,
                    "local preview mode active (read-only, no GitHub)"
                );
                // Preview has no OAuth login, but `login_org()` is read while
                // building AppConfig during SSR; initialize it org-less so it is
                // never an uninitialized panic.
                auth::init_login_org(&target.org, true).unwrap_or_else(|error| {
                    tracing::error!(%error, "failed to initialize login organization policy");
                    std::process::exit(1)
                });
                target
            }
            Err(message) => {
                eprintln!("error: {message}");
                std::process::exit(1);
            }
        }
    } else {
        let target_bootstrap = gitnodes_app::server::runtime_config::target_from_env_or_exit();
        auth::init_login_org(
            &target_bootstrap.target.org,
            target_bootstrap.compact_locator,
        )
        .unwrap_or_else(|error| {
            tracing::error!(%error, "failed to initialize login organization policy");
            std::process::exit(1)
        });
        target_bootstrap.target
    };
    let brand_cfg = gitnodes_app::server::runtime_config::brand_from_env(&target_cfg);

    // Single pooled, **target-agnostic** HTTP client for the whole process.
    // Threaded through Leptos context so server fns and the asset proxy share
    // connection state. The transport carries no target binding; each call
    // site supplies the right `TargetConfig` per request — that's what keeps
    // a future Brain-Switcher (Phase 3) from silently reading the wrong repo.
    let gh_http =
        gitnodes_storage::GithubHttp::new().expect("failed to build pooled GitHub HTTP client");
    tracing::info!("github http client built (pooled, target-agnostic)");

    // Runtime store backed by SQLite. Normal servers persist sessions, audit
    // events, and projections; preview keeps the same schema entirely in memory.
    let db_url = if local_preview {
        "sqlite::memory:".to_string()
    } else {
        std::env::var("SESSION_DB_URL").unwrap_or_else(|_| "sqlite://data/sessions.db".to_string())
    };

    // Only attempt to create parent directories if it's a local SQLite file
    if db_url.starts_with("sqlite://") && !db_url.starts_with("sqlite://:memory:") {
        let file_path = db_url
            .strip_prefix("sqlite://")
            .expect("db_url starts_with sqlite:// guard above guarantees this prefix");
        if let Some(parent) = std::path::Path::new(file_path).parent()
            && !parent.as_os_str().is_empty()
        {
            // Do not swallow the error! Fail fast if permissions are wrong.
            if let Err(e) = std::fs::create_dir_all(parent) {
                tracing::error!(
                    parent = ?parent,
                    error = %e,
                    "failed to create session DB directory"
                );
                std::process::exit(1);
            }
        }
    }

    let sqlite_opts = SqliteConnectOptions::from_str(&db_url)
        .expect("Valid database connection string")
        .create_if_missing(true)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        // A SQLite in-memory database is connection-local. Preview uses one
        // pooled connection so sessions and the projection see the same schema.
        .max_connections(if local_preview { 1 } else { 5 })
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
    gitnodes_app::server::projection::migrate(&pool)
        .await
        .expect("projection table migration");
    gitnodes_app::server::audit::init(pool.clone());
    gitnodes_app::server::projection::init(pool.clone());

    // Preview mode seeds the projection from the working tree now that the pool
    // exists, the same way the read-only MCP server does.
    if local_preview
        && let Err(message) = gitnodes_app::server::local::rebuild_projection("preview-boot").await
    {
        eprintln!("error: failed to index local working tree: {message}");
        std::process::exit(1);
    }

    let event_bus = gitnodes_app::server::sse::EventBus::new();
    gitnodes_app::server::sse::init(event_bus.clone());

    if !local_preview {
        // Slice γ: supervised background retry for the provider-sync outbox.
        gitnodes_app::server::pending_sync_job::spawn(pool.clone(), gh_http.clone());

        // Schema v2: supervised retention for persistent runtime state.
        gitnodes_app::server::retention::spawn(pool.clone());
    }

    let allow_insecure_webhooks = std::env::var("ALLOW_INSECURE_WEBHOOKS")
        .map(|v| v != "0" && !v.is_empty())
        .unwrap_or(cfg!(debug_assertions));
    let webhook_auth = match std::env::var("WEBHOOK_SECRET") {
        _ if local_preview => gitnodes_app::server::webhook::WebhookAuth::Disabled,
        Ok(secret) if !secret.is_empty() => {
            gitnodes_app::server::webhook::WebhookAuth::Secret(secret)
        }
        _ if allow_insecure_webhooks => {
            tracing::warn!(
                "webhook endpoint accepts unsigned payloads because ALLOW_INSECURE_WEBHOOKS is enabled"
            );
            gitnodes_app::server::webhook::WebhookAuth::Insecure
        }
        _ => {
            tracing::info!("webhook endpoint disabled; set WEBHOOK_SECRET to enable it");
            gitnodes_app::server::webhook::WebhookAuth::Disabled
        }
    };
    let webhook_state = gitnodes_app::server::webhook::WebhookState {
        bus: event_bus.clone(),
        http: gh_http.clone(),
        auth: webhook_auth,
    };
    // OAuth callback is a cross-site redirect back from github.com, so the session
    // cookie must be SameSite=Lax (Strict would drop it and kill CSRF state check).
    // Secure=false allows http://127.0.0.1 in dev; set SESSION_COOKIE_SECURE=1 in prod.
    let cookie_secure = std::env::var("SESSION_COOKIE_SECURE")
        .map(|v| v != "0" && !v.is_empty())
        .unwrap_or(!cfg!(debug_assertions));

    // Encrypt OAuth tokens in the session store. An explicit env key wins;
    // otherwise a private key is generated once under data/ and reused. Preview
    // mode uses an ephemeral in-memory key so it never writes into the user's
    // content directory (sessions are anonymous and read-only anyway).
    let session_key = if local_preview {
        tower_sessions::cookie::Key::generate()
    } else {
        gitnodes_app::server::session_key::load().unwrap_or_else(|error| {
            tracing::error!(%error, "failed to load session encryption key");
            std::process::exit(1)
        })
    };
    let session_layer = SessionManagerLayer::new(session_store)
        .with_same_site(SameSite::Lax)
        .with_secure(cookie_secure)
        .with_private(session_key);

    // Single-binary builds carry their web assets inside the executable. Unpack
    // them to a cache dir and point Leptos at it before the config is read.
    #[cfg(feature = "embed-assets")]
    match gitnodes_app::server::embedded::extract_site() {
        Ok(site) => {
            // SAFETY: startup is single-threaded here, before any config read or
            // task spawn observes the environment.
            unsafe { std::env::set_var("LEPTOS_SITE_ROOT", &site) };
            tracing::info!(site = %site.display(), "serving embedded web assets");
        }
        Err(error) => {
            tracing::error!(%error, "failed to extract embedded web assets");
            std::process::exit(1);
        }
    }

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

    // Preview and PAT mode are mutually exclusive auth postures. Preview serves
    // read-only and unauthenticated, so it only enforces the loopback guardrail;
    // otherwise resolve single-user PAT mode (validates the token, records the
    // operator identity, enforces the same guardrail) before serving.
    if local_preview {
        gitnodes_app::server::local::enforce_loopback(&addr);
    } else {
        gitnodes_app::server::pat::init(&addr).await;
    }

    let routes = generate_route_list(App);

    // Path-aware auth gate: blocks anything under `/knowledge` for anonymous users.
    // SSE is also gated — without it, anyone can subscribe to `/sse/events` and
    // infer private repo activity (push timing, rebuild failures) from the
    // typed event names. SSE gets `401` instead of a redirect because
    // `EventSource` would otherwise treat the redirect as success and
    // reconnect-loop forever.
    async fn protect_knowledge(session: Session, request: Request<Body>, next: Next) -> Response {
        let path = request.uri().path();
        // Preview mode is read-only with no forge: the landing route has no
        // login to show, and the admin/operator/PR surfaces have no remote to
        // act on. Bounce all of them to the graph server-side (covers every
        // route variant, 3- and 4-segment), so these surfaces are hidden rather
        // than rendering a raw authorization error.
        if gitnodes_app::server::local::is_enabled() {
            let is_forge_surface = path
                .trim_start_matches('/')
                .split('/')
                .any(|segment| matches!(segment, "admin" | "pulls"));
            if path == "/" || is_forge_surface {
                return Redirect::to("/knowledge").into_response();
            }
        }
        if is_protected_path(path) && !auth::is_authenticated(&session).await {
            if path == "/sse" || path.starts_with("/sse/") {
                axum::http::StatusCode::UNAUTHORIZED.into_response()
            } else {
                Redirect::to("/").into_response()
            }
        } else {
            next.run(request).await
        }
    }

    async fn cache_control(request: Request<Body>, next: Next) -> Response {
        let path = request.uri().path().to_string();
        let mut response = next.run(request).await;
        let headers = response.headers_mut();

        if path.starts_with("/pkg/") {
            // Build outputs use stable filenames (`gitnodes.js`, `gitnodes.wasm`,
            // `gitnodes.css`), so browsers must revalidate them on every load
            // or a new SSR HTML page can hydrate against an older client bundle.
            headers.insert(
                CACHE_CONTROL,
                "no-cache, no-store, must-revalidate"
                    .parse()
                    .expect("valid cache-control header"),
            );
            headers.insert(PRAGMA, "no-cache".parse().expect("valid pragma header"));
        } else if !path.starts_with("/assets/")
            && !path.starts_with("/api/")
            && !path.starts_with("/sse/")
            && !path.starts_with("/webhook/")
        {
            headers.insert(
                CACHE_CONTROL,
                "no-cache, no-store, must-revalidate"
                    .parse()
                    .expect("valid cache-control header"),
            );
            headers.insert(PRAGMA, "no-cache".parse().expect("valid pragma header"));
        }

        response
    }

    // Baseline browser-side hardening. The CSP `frame-src 'none'` is static for
    // now and will become dynamic once the embed allowlist exists.
    // HSTS is only emitted when the app is actually serving over HTTPS
    // (signalled by `SESSION_COOKIE_SECURE`); on local http://127.0.0.1
    // dev it would force the browser to https and break the dev loop.
    // Static CSP baseline. Mermaid is self-hosted under `/vendor`, so script
    // loading stays same-origin. `img-src` includes `data:`/`blob:` for
    // inline/preview images and `https:` so GitHub `raw.githubusercontent.com`
    // images (non-asset markdown images rewritten to raw URLs) load; private-repo
    // assets go through the same-origin proxy. `connect-src` covers the SSE
    // endpoint and dev ws.
    const CSP_POLICY: &str = "default-src 'self'; \
script-src 'self' 'unsafe-inline' 'wasm-unsafe-eval'; \
style-src 'self' 'unsafe-inline'; \
img-src 'self' data: blob: https:; \
font-src 'self' data:; \
connect-src 'self' ws: wss:; \
frame-src 'none'; \
object-src 'none'; \
base-uri 'none'; \
frame-ancestors 'none'; \
form-action 'self'";

    // Locked-down CSP for proxied asset responses. Asset bytes are untrusted
    // repo content served same-origin; an SVG opened as a top-level document
    // would otherwise execute inline scripts under `CSP_POLICY` (which must keep
    // `script-src 'unsafe-inline'` for Leptos hydration). `sandbox` (no
    // `allow-scripts`) and `default-src 'none'` both block script execution
    // while still letting <img>-embedded assets render — those never run SVG
    // scripts regardless of CSP. Mirrors the upload allowlist, which already
    // rejects SVG (see `file_ops::is_allowed_image_ext`).
    const ASSET_CSP_POLICY: &str =
        "default-src 'none'; img-src 'self' data:; style-src 'unsafe-inline'; sandbox";

    async fn security_headers(cookie_secure: bool, request: Request<Body>, next: Next) -> Response {
        // Captured before the request is consumed and dispatched into the asset
        // nest; an outer layer still sees the full original path here.
        let asset_response = is_asset_path(request.uri().path());
        let mut response = next.run(request).await;
        let headers = response.headers_mut();
        headers.insert(
            "X-Content-Type-Options",
            HeaderValue::from_static("nosniff"),
        );
        headers.insert("X-Frame-Options", HeaderValue::from_static("DENY"));
        headers.insert(
            "Referrer-Policy",
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        );
        // Content-Security-Policy baseline. `frame-src 'none'` because no iframes
        // exist yet — when the embed allowlist lands it becomes dynamic. We keep
        // `script-src`/`style-src` permissive enough for Leptos hydration:
        // `HydrationScripts` emits an inline bootstrap, the WASM module needs
        // `wasm-unsafe-eval`, and Tailwind/Leptos inject inline styles.
        // Tightening to nonces is tracked as a future hardening step.
        // `connect-src` allows ws: in dev for AutoReload/live reload.
        headers.insert(
            "Content-Security-Policy",
            HeaderValue::from_static(if asset_response {
                ASSET_CSP_POLICY
            } else {
                CSP_POLICY
            }),
        );
        if cookie_secure {
            headers.insert(
                "Strict-Transport-Security",
                HeaderValue::from_static("max-age=31536000; includeSubDomains"),
            );
        }
        response
    }

    // Same-origin guard for state-changing requests. Authenticated mutations run
    // over `POST /api/*`; SameSite=Lax already blocks cross-site cookie sends for
    // top-level POSTs, but we additionally reject any mutating `/api/*` request
    // whose `Origin` host doesn't match the request `Host`. This closes the gap
    // before iframes/automations exist. The webhook (`/webhook/github`, HMAC) and
    // OAuth callback (`/auth/callback`, GET, cross-site by design) are exempt
    // because they aren't `/api/*` POSTs.
    async fn csrf_protect(request: Request<Body>, next: Next) -> Response {
        use axum::http::{StatusCode, header};

        let method = request.method();
        let is_mutating = matches!(method.as_str(), "POST" | "PUT" | "PATCH" | "DELETE");
        let path = request.uri().path();
        if is_mutating && path.starts_with("/api/") {
            let headers = request.headers();
            let host = headers.get(header::HOST).and_then(|h| h.to_str().ok());
            let origin = headers.get(header::ORIGIN).and_then(|h| h.to_str().ok());
            let sec_fetch_site = headers.get("sec-fetch-site").and_then(|h| h.to_str().ok());
            if !is_same_origin(host, origin, sec_fetch_site) {
                return (StatusCode::FORBIDDEN, "cross-site request rejected").into_response();
            }
        }
        next.run(request).await
    }

    let options_for_ssr = leptos_options.clone();

    // Per-IP rate-limit baseline. Generous so a normal contributor session never
    // trips it; the point is to blunt brute-force/enumeration on /auth/callback,
    // /webhook/github and abuse of the mutating server fns. Limits are overridable
    // via env for ops tuning. `SmartIpKeyExtractor` reads X-Forwarded-For/X-Real-IP
    // (correct behind Railway's proxy) and falls back to the peer IP.
    let rl_per_second: u64 = std::env::var("RATE_LIMIT_PER_SECOND")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);
    let rl_burst: u32 = std::env::var("RATE_LIMIT_BURST")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let governor_conf = std::sync::Arc::new(
        tower_governor::governor::GovernorConfigBuilder::default()
            .per_second(rl_per_second)
            .burst_size(rl_burst)
            .key_extractor(tower_governor::key_extractor::SmartIpKeyExtractor)
            .finish()
            .expect("valid rate-limit config"),
    );
    let governor_layer = tower_governor::GovernorLayer {
        config: governor_conf,
    };

    // Private-repo asset proxy. Raw GitHub URLs would require the user's OAuth
    // token on `<img>` requests, which the browser can't attach — so we serve
    // bytes ourselves via the Contents API with the session's token.
    let asset_router = Router::new()
        .route(
            "/{*path}",
            axum::routing::get(gitnodes_app::server::assets::serve_asset),
        )
        .with_state(gitnodes_app::server::assets::AssetProxyState {
            http: gh_http.clone(),
            target: target_cfg.clone(),
        });

    let app = Router::new()
        .nest("/assets", asset_router.clone())
        .nest("/{org}/{repo}/assets", asset_router)
        // Operational probes. Unauthenticated by design: `is_protected_path`
        // returns false for these, and CSRF only gates `/api/*` mutations. They
        // still pass through the outer governor rate limiter, but orchestrator
        // probes are infrequent and well under the default burst (60).
        .route(
            "/healthz",
            axum::routing::get(gitnodes_app::server::health::healthz),
        )
        .route(
            "/readyz",
            axum::routing::get(gitnodes_app::server::health::readyz),
        )
        .route("/auth/login", axum::routing::get(auth::login))
        .route("/auth/logout", axum::routing::get(auth::logout))
        .route("/auth/callback", axum::routing::get(auth::oauth_callback))
        .route(
            "/sse/events",
            axum::routing::get(gitnodes_app::server::sse::handle).with_state(
                gitnodes_app::server::sse::SseState {
                    bus: event_bus.clone(),
                    default_target: target_cfg.clone(),
                    http: gh_http.clone(),
                },
            ),
        )
        .route(
            "/webhook/github",
            axum::routing::post(gitnodes_app::server::webhook::handle).with_state(webhook_state),
        )
        // Server functions: extract Session and inject Session + runtime config
        // into Leptos context so use_context::<...>() works inside #[server] fns.
        // 3.7B-α: target identity is resolved from the URL path via the
        // `routing` module — never from `Referer`. Mutations carry an
        // explicit `TargetRef` in the body; reads inherit the path-derived
        // context.
        .route(
            "/api/{*fn_name}",
            axum::routing::post({
                let target_for_api = target_cfg.clone();
                let brand_for_api = brand_cfg.clone();
                let http_for_api = gh_http.clone();
                move |session: Session, request: Request<Body>| {
                    let fallback = target_for_api.clone();
                    let brand = brand_for_api.clone();
                    let http = http_for_api.clone();
                    async move {
                        let path = request.uri().path().to_string();
                        let resolved = gitnodes_app::server::routing::resolve_path(
                            &path,
                            &fallback,
                            gitnodes_app::server::projection::pool_handle(),
                        )
                        .await;
                        let target = resolved.target_config(&fallback);
                        let legacy_marker = resolved.legacy_marker();
                        leptos_axum::handle_server_fns_with_context(
                            move || {
                                provide_context(session.clone());
                                provide_context(target.clone());
                                provide_context(brand.clone());
                                provide_context(http.clone());
                                if let Some(ref marker) = legacy_marker {
                                    provide_context(marker.clone());
                                }
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
                let fallback = target_for_ssr.clone();
                let brand = brand_for_ssr.clone();
                let http = http_for_ssr.clone();
                async move {
                    let path = request.uri().path().to_string();
                    let resolved = gitnodes_app::server::routing::resolve_path(
                        &path,
                        &fallback,
                        gitnodes_app::server::projection::pool_handle(),
                    )
                    .await;
                    let target = resolved.target_config(&fallback);
                    let legacy_marker = resolved.legacy_marker();
                    let handler = leptos_axum::render_app_to_stream_with_context(
                        move || {
                            provide_context(session.clone());
                            provide_context(target.clone());
                            provide_context(brand.clone());
                            provide_context(http.clone());
                            if let Some(ref marker) = legacy_marker {
                                provide_context(marker.clone());
                            }
                        },
                        move || shell(options.clone()),
                    );
                    handler(request).await
                }
            }
        })
        .fallback(leptos_axum::file_and_error_handler(shell))
        // Layer order matters. In axum, later `.layer()` calls become OUTER
        // wrappers. We want `security_headers` (and `cache_control`) to decorate
        // ALL responses — including the redirect/401 short-circuits emitted by
        // `protect_knowledge` itself — so they must wrap that middleware.
        // Registration order therefore goes innermost → outermost:
        //   protect_knowledge  → cache_control  → security_headers  → session.
        .layer(middleware::from_fn(protect_knowledge))
        .layer(middleware::from_fn(csrf_protect))
        .layer(middleware::from_fn(cache_control))
        .layer(middleware::from_fn(move |req, next| {
            security_headers(cookie_secure, req, next)
        }))
        // Hard backstop on request body size. The largest legitimate body is a
        // base64/JSON-encoded asset upload (`MAX_ASSET_BYTES` raw, ~33% larger
        // once JSON-encoded); 8 MiB leaves generous headroom while rejecting
        // multi-megabyte payloads before they hit a server fn.
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024))
        .layer(session_layer)
        // Outermost: reject over-limit requests before any session/DB work.
        .layer(governor_layer)
        .with_state(leptos_options);

    tracing::info!(%addr, "gitnodes_app listening");
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(%addr, error = %e, "failed to bind TCP listener");
            std::process::exit(1);
        }
    };
    // Local dev convenience: pop open the browser at the knowledge view. Only on
    // a loopback bind (skip server/container deployments), and suppressible.
    let url = format!("http://{addr}/knowledge");
    tracing::info!("GitNodes ready at {url}");
    if addr.ip().is_loopback() && std::env::var("GITNODES_NO_OPEN").is_err() {
        gitnodes_app::cli::open_browser(&url);
    }
    // `into_make_service_with_connect_info` exposes the peer `SocketAddr` so the
    // rate limiter's `SmartIpKeyExtractor` has a fallback when no proxy headers
    // are present (e.g. local dev).
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .expect("axum serve loop terminated with error");
}

#[cfg(not(feature = "ssr"))]
pub fn main() {
    // no client-side main function
    // see lib.rs for hydration function instead
}
