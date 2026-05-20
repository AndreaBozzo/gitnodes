#![recursion_limit = "512"]
#![cfg_attr(not(test), warn(clippy::unwrap_used))]

#[cfg(feature = "ssr")]
fn required_env_with_legacy(primary: &str, legacy: &str) -> String {
    std::env::var(primary)
        .or_else(|_| std::env::var(legacy))
        .unwrap_or_else(|_| {
            tracing::error!(
                "missing required environment variable: set {primary} (or legacy {legacy})"
            );
            std::process::exit(1)
        })
}

#[cfg(feature = "ssr")]
fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        tracing::error!("missing required environment variable: {name}");
        std::process::exit(1)
    })
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
    use super::{is_protected_path, is_same_origin};

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
            ("/auth/login", false),
            ("/auth/callback", false),
            ("/webhook/github", false),
            ("/api/get_current_user", false),
            ("/pkg/brain_ui.js", false),
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
#[tokio::main]
async fn main() {
    use axum::{
        Router,
        body::Body,
        extract::{DefaultBodyLimit, Request},
        http::HeaderValue,
        http::header::{CACHE_CONTROL, PRAGMA},
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
                .unwrap_or_else(|_| "brain_app=info,warn".into()),
        )
        .init();

    // Runtime config from env — fail fast if any are missing so a misconfigured
    // deploy can't silently write to the wrong repo.
    let target_cfg = TargetConfig {
        org: required_env_with_legacy("TARGET_GITHUB_ORG", "GITHUB_ORG"),
        repo: required_env_with_legacy("TARGET_GITHUB_REPO", "GITHUB_REPO"),
        branch: required_env_with_legacy("TARGET_GITHUB_BRANCH", "GITHUB_BRANCH"),
    };
    let brand_cfg = BrandConfig {
        name: required_env("BRAND_NAME"),
        org_label: required_env("BRAND_ORG_LABEL"),
    };

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
    brain_app::server::sse::init(event_bus.clone());

    let allow_insecure_webhooks = std::env::var("ALLOW_INSECURE_WEBHOOKS")
        .map(|v| v != "0" && !v.is_empty())
        .unwrap_or(cfg!(debug_assertions));
    let webhook_secret = std::env::var("WEBHOOK_SECRET").ok();
    if webhook_secret.is_none() {
        if allow_insecure_webhooks {
            tracing::warn!(
                "WEBHOOK_SECRET not set — webhook endpoint will accept unsigned payloads because ALLOW_INSECURE_WEBHOOKS is enabled"
            );
        } else {
            tracing::error!(
                "WEBHOOK_SECRET must be set in non-dev environments unless ALLOW_INSECURE_WEBHOOKS=1"
            );
            std::process::exit(1);
        }
    }
    let webhook_state = brain_app::server::webhook::WebhookState {
        bus: event_bus.clone(),
        http: gh_http.clone(),
        secret: webhook_secret,
    };
    // OAuth callback is a cross-site redirect back from github.com, so the session
    // cookie must be SameSite=Lax (Strict would drop it and kill CSRF state check).
    // Secure=false allows http://127.0.0.1 in dev; set SESSION_COOKIE_SECURE=1 in prod.
    let cookie_secure = std::env::var("SESSION_COOKIE_SECURE")
        .map(|v| v != "0" && !v.is_empty())
        .unwrap_or(!cfg!(debug_assertions));

    // Encrypt the session store at rest. The SQLite session record holds the
    // user's `gho_*` OAuth token; `with_private` keeps it ciphertext on disk.
    // Key is a base64-encoded >=64-byte secret in `SESSION_ENCRYPTION_KEY`.
    // In prod (cookie_secure) the key is mandatory — mirror the WEBHOOK_SECRET
    // fail-fast. In dev a missing key generates an ephemeral one (sessions don't
    // survive a restart, which is fine locally).
    let session_key = match std::env::var("SESSION_ENCRYPTION_KEY") {
        Ok(b64) => {
            use base64::Engine as _;
            // Strip ALL whitespace, not just the ends: `openssl rand -base64 64`
            // wraps its output at 64 cols, so a pasted value often carries an
            // internal newline that `trim()` alone would leave in place.
            let cleaned: String = b64.chars().filter(|c| !c.is_whitespace()).collect();
            let raw = base64::engine::general_purpose::STANDARD
                .decode(&cleaned)
                .unwrap_or_else(|e| {
                    tracing::error!(error = %e, "SESSION_ENCRYPTION_KEY is not valid base64");
                    std::process::exit(1);
                });
            tower_sessions::cookie::Key::try_from(raw.as_slice()).unwrap_or_else(|e| {
                tracing::error!(error = %e, "SESSION_ENCRYPTION_KEY must decode to >= 64 bytes");
                std::process::exit(1);
            })
        }
        Err(_) => {
            if cookie_secure {
                tracing::error!(
                    "SESSION_ENCRYPTION_KEY must be set in production (base64, >= 64 bytes). \
                     Generate one with: openssl rand -base64 64 | tr -d '\\n'"
                );
                std::process::exit(1);
            }
            tracing::warn!(
                "SESSION_ENCRYPTION_KEY not set — generating an ephemeral dev key; \
                 sessions will not survive a restart"
            );
            tower_sessions::cookie::Key::generate()
        }
    };
    let session_layer = SessionManagerLayer::new(session_store)
        .with_same_site(SameSite::Lax)
        .with_secure(cookie_secure)
        .with_private(session_key);

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
            // Build outputs use stable filenames (`brain_ui.js`, `brain_ui.wasm`,
            // `brain_ui.css`), so browsers must revalidate them on every load
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
    // Static CSP baseline. `img-src` includes `data:`/`blob:` for inline/preview
    // images and `https:` so GitHub `raw.githubusercontent.com` images (non-asset
    // markdown images rewritten to raw URLs) load; private-repo assets go through
    // the same-origin proxy. `connect-src` covers the SSE endpoint and dev ws.
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

    async fn security_headers(cookie_secure: bool, request: Request<Body>, next: Next) -> Response {
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
        // `HydrationScripts` emits an inline bootstrap and the WASM module needs
        // `wasm-unsafe-eval`; Tailwind/Leptos inject inline styles. This still
        // blocks the primary XSS vector (loading script from a foreign origin).
        // Tightening to nonces is tracked as a future hardening step.
        // `connect-src` allows ws: in dev for AutoReload/live reload.
        headers.insert(
            "Content-Security-Policy",
            HeaderValue::from_static(CSP_POLICY),
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
            axum::routing::get(brain_app::server::assets::serve_asset),
        )
        .with_state(brain_app::server::assets::AssetProxyState {
            http: gh_http.clone(),
            target: target_cfg.clone(),
        });

    let app = Router::new()
        .nest("/assets", asset_router.clone())
        .nest("/{org}/{repo}/assets", asset_router)
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
                        let resolved = brain_app::server::routing::resolve_path(
                            &path,
                            &fallback,
                            brain_app::server::projection::pool_handle(),
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
                    let resolved = brain_app::server::routing::resolve_path(
                        &path,
                        &fallback,
                        brain_app::server::projection::pool_handle(),
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

    tracing::info!(%addr, "brain_app listening");
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(%addr, error = %e, "failed to bind TCP listener");
            std::process::exit(1);
        }
    };
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
