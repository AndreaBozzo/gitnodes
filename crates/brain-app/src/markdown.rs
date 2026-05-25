//! Shared markdown helpers used by both SSR (rendering persisted files) and
//! the client-side hydrate build (live preview in the editor).

use brain_domain::{GithubClient, TargetConfig, encode_path_segment};
use std::path::{Component, Path};

/// Split a YAML frontmatter block off the top of a markdown file.
/// Returns `(body_without_frontmatter, raw_frontmatter_text)`.
pub fn split_frontmatter(src: &str) -> (&str, Option<&str>) {
    let rest = match src.strip_prefix("---\n") {
        Some(r) => r,
        None => match src.strip_prefix("---\r\n") {
            Some(r) => r,
            None => return (src, None),
        },
    };
    if let Some(end) = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n")) {
        let frontmatter = &rest[..end];
        let after = &rest[end..];
        let body = after
            .strip_prefix("\n---\n")
            .or_else(|| after.strip_prefix("\n---\r\n"))
            .unwrap_or(after);
        (body, Some(frontmatter))
    } else {
        (src, None)
    }
}

/// Render a markdown body to HTML using pulldown-cmark with CommonMark extensions.
/// No link rewriting — use for previews where the body isn't tied to a repo file.
pub fn render(body: &str) -> String {
    render_for_path(body, None, None)
}

/// Render markdown for a persisted Brain file, rewriting repo-relative links to
/// app routes or GitHub URLs (scoped to `cfg`) so they resolve correctly in the UI.
pub fn render_for_file(body: &str, file_path: &str, cfg: &TargetConfig) -> String {
    render_for_path(body, Some(file_path), Some(cfg))
}

fn render_for_path(body: &str, file_path: Option<&str>, cfg: Option<&TargetConfig>) -> String {
    use pulldown_cmark::{Options, Parser, html};

    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_SMART_PUNCTUATION);
    let parser = Parser::new_ext(body, opts).map(|event| rewrite_event(event, file_path, cfg));
    let mut out = String::with_capacity(body.len() + body.len() / 4);
    html::push_html(&mut out, parser);
    sanitize(out)
}

/// Server-side defense-in-depth: run the generated HTML through ammonia with a
/// tight allowlist so any HTML that slipped through (raw HTML in the source,
/// crafted attributes) can't carry script/`on*`/`javascript:`/`iframe`. This is
/// the real trust boundary for Brain docs and GitHub comments rendered into the
/// detail panel via `inner_html`.
///
/// `ammonia` pulls in `html5ever`, which does not build for `wasm32`, so it is
/// only compiled for the `ssr` server build. Raw HTML tags are already escaped
/// upstream in `rewrite_event` (WASM-safe), so the client preview stays
/// consistent with the sanitized server output without needing ammonia.
#[cfg(feature = "ssr")]
fn sanitize(html: String) -> String {
    use ammonia::Builder;
    use std::collections::HashSet;
    use std::sync::OnceLock;

    static CLEANER: OnceLock<Builder<'static>> = OnceLock::new();
    let cleaner = CLEANER.get_or_init(|| {
        let mut b = Builder::default();
        // Only http(s)/mailto/tel links survive; `javascript:` and friends are
        // dropped. Repo-relative links were already rewritten to `/knowledge?…`
        // and `/{org}/{repo}/assets/…` (relative URLs, allowed by default).
        let schemes: HashSet<&str> = ["http", "https", "mailto", "tel"].into_iter().collect();
        b.url_schemes(schemes);

        let mut allowed_classes = std::collections::HashMap::new();
        let mut code_classes = std::collections::HashSet::new();
        code_classes.insert("language-mermaid");
        code_classes.insert("mermaid");
        // Also allow common syntax highlighting classes
        for lang in &[
            "rust",
            "javascript",
            "typescript",
            "css",
            "html",
            "json",
            "yaml",
            "markdown",
            "bash",
            "sh",
        ] {
            code_classes.insert(lang);
            let lang_class = format!("language-{lang}");
            // Leak to 'static str to satisfy Ammonia's life-time requirements
            let leaked: &'static str = Box::leak(lang_class.into_boxed_str());
            code_classes.insert(leaked);
        }
        allowed_classes.insert("code", code_classes);
        b.allowed_classes(allowed_classes);

        b
    });
    cleaner.clean(&html).to_string()
}

/// Client (WASM) build has no ammonia. Raw HTML is escaped upstream in
/// `rewrite_event`, so the preview HTML is already free of active markup; this
/// is a passthrough that keeps `render_for_path` identical across builds.
#[cfg(not(feature = "ssr"))]
fn sanitize(html: String) -> String {
    html
}

fn rewrite_event<'a>(
    event: pulldown_cmark::Event<'a>,
    file_path: Option<&str>,
    cfg: Option<&TargetConfig>,
) -> pulldown_cmark::Event<'a> {
    use pulldown_cmark::{CowStr, Event, Tag};

    match event {
        // Drop raw HTML to plain text so `html::push_html` escapes it. This
        // neutralizes `<script>`, `<iframe>`, `<img onerror=…>` etc. before they
        // reach the DOM, on both the server render and the WASM editor preview
        // (where ammonia is unavailable). Server output is additionally cleaned
        // by `sanitize()`.
        Event::Html(raw) | Event::InlineHtml(raw) => Event::Text(raw),
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Link {
            link_type,
            dest_url: CowStr::Boxed(
                rewrite_link_destination(dest_url.as_ref(), file_path, cfg, false).into_boxed_str(),
            ),
            title,
            id,
        }),
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Image {
            link_type,
            dest_url: CowStr::Boxed(
                rewrite_link_destination(dest_url.as_ref(), file_path, cfg, true).into_boxed_str(),
            ),
            title,
            id,
        }),
        other => other,
    }
}

fn rewrite_link_destination(
    dest: &str,
    file_path: Option<&str>,
    cfg: Option<&TargetConfig>,
    is_image: bool,
) -> String {
    // Neutralize any link carrying a scheme that isn't on the safe allowlist
    // (e.g. `javascript:`, `data:`, `vbscript:`). This runs in BOTH the SSR and
    // WASM builds, so the editor preview — which renders via `inner_html`
    // without ammonia — can't execute `[x](javascript:alert(1))`.
    if let Some(scheme) = url_scheme(dest) {
        if is_safe_scheme(scheme) {
            return dest.to_string();
        }
        return "#".to_string();
    }

    if dest.is_empty() || dest.starts_with('#') {
        return dest.to_string();
    }

    if is_app_route(dest) {
        return dest.to_string();
    }

    let (path_part, fragment) = split_fragment(dest);
    let resolved = resolve_repo_path(file_path, path_part);

    if resolved.ends_with(".md") {
        let mut url = if let Some(cfg) = cfg {
            format!(
                "/{}/{}/{}/knowledge?path={}",
                cfg.org,
                cfg.repo,
                encode_path_segment(&cfg.branch),
                encode_query_value(&resolved)
            )
        } else {
            format!("/knowledge?path={}", encode_query_value(&resolved))
        };
        if let Some(fragment) = fragment {
            url.push('#');
            url.push_str(fragment);
        }
        return url;
    }

    // Images under `assets/` go through our authenticated proxy so private-repo
    // bytes reach the browser without needing an OAuth token on `<img>`.
    if is_image && resolved.starts_with("assets/") {
        let mut url = cfg
            .map(|cfg| format!("/{}/{}/{}", cfg.org, cfg.repo, resolved))
            .unwrap_or_else(|| format!("/{}", resolved));
        if let Some(fragment) = fragment {
            url.push('#');
            url.push_str(fragment);
        }
        return url;
    }

    // Without a config, we can't build absolute GitHub URLs — leave as-is.
    let Some(cfg) = cfg else {
        return dest.to_string();
    };
    let gh = GithubClient::new(cfg.clone());
    let base = if is_image {
        gh.raw_base()
    } else {
        gh.blob_base()
    };
    let mut url = format!("{}/{}", base, resolved);
    if let Some(fragment) = fragment {
        url.push('#');
        url.push_str(fragment);
    }
    url
}

/// Extract the URL scheme (the part before `:`) if `dest` is an absolute URL.
/// Returns `None` for relative paths, fragments, and protocol-relative `//`
/// links. A scheme is `ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )` per RFC 3986
/// and must come before any `/`, `?`, or `#`.
fn url_scheme(dest: &str) -> Option<&str> {
    let colon = dest.find(':')?;
    let scheme = &dest[..colon];
    if scheme.is_empty() {
        return None;
    }
    // A `:` appearing after a path separator isn't a scheme (e.g. `foo/bar:baz`).
    if scheme.contains(['/', '?', '#']) {
        return None;
    }
    let mut chars = scheme.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    if chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        Some(scheme)
    } else {
        None
    }
}

fn is_safe_scheme(scheme: &str) -> bool {
    matches!(
        scheme.to_ascii_lowercase().as_str(),
        "http" | "https" | "mailto" | "tel"
    )
}

fn is_app_route(dest: &str) -> bool {
    dest == "/"
        || dest.starts_with("/knowledge")
        || dest.starts_with("/admin")
        || dest.starts_with("/auth/")
        || dest.starts_with("/api/")
        || dest.starts_with("/pkg/")
}

fn split_fragment(dest: &str) -> (&str, Option<&str>) {
    match dest.split_once('#') {
        Some((path, fragment)) => (path, Some(fragment)),
        None => (dest, None),
    }
}

fn resolve_repo_path(file_path: Option<&str>, dest: &str) -> String {
    let joined = if let Some(stripped) = dest.strip_prefix('/') {
        stripped.to_string()
    } else if let Some(base) = file_path {
        let parent = Path::new(base).parent().unwrap_or_else(|| Path::new(""));
        let joined = parent.join(dest);
        joined.to_string_lossy().into_owned()
    } else {
        dest.to_string()
    };

    normalize_repo_path(&joined)
}

/// Resolve a frontmatter `cover:` value into a browser-usable image URL.
///
/// `cover` may be one of:
/// - an absolute http(s) URL: returned untouched
/// - a repo-rooted `assets/...` path: routed through the authenticated proxy
/// - any other repo path (relative or rooted): resolved to `raw.githubusercontent.com`
///
/// `file_path` is the markdown file the cover belongs to — used to resolve
/// relative paths like `../assets/foo.png` exactly like inline image links.
/// Dangerous schemes (`javascript:`, `data:`, `vbscript:`) yield `None` so the
/// caller can omit the hero block entirely.
pub fn resolve_cover_url(cover: &str, file_path: &str, cfg: &TargetConfig) -> Option<String> {
    let trimmed = cover.trim();
    if trimmed.is_empty() {
        return None;
    }
    let rewritten = rewrite_link_destination(trimmed, Some(file_path), Some(cfg), true);
    if rewritten == "#" {
        // The destination carried a dangerous scheme; drop the cover rather
        // than emit a placeholder anchor as an image src.
        return None;
    }
    Some(rewritten)
}

/// Build a markdown link target from a repo-rooted file to a repo-rooted asset.
///
/// When the file path is unknown, fall back to the app's asset route so live
/// preview can still display an uploaded image before a new note has a title.
pub fn repo_relative_link_target(file_path: Option<&str>, target: &str) -> String {
    let target = normalize_repo_path(target);
    let Some(file_path) = file_path else {
        return format!("/{target}");
    };
    let from_dir = Path::new(file_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let from_parts: Vec<String> = from_dir
        .iter()
        .filter_map(|p| p.to_str())
        .filter(|p| !p.is_empty() && *p != ".")
        .map(ToOwned::to_owned)
        .collect();
    let target_parts: Vec<&str> = target.split('/').filter(|s| !s.is_empty()).collect();

    if target_parts.is_empty() {
        return String::new();
    }

    let mut common = 0;
    while common < from_parts.len()
        && common < target_parts.len() - 1
        && from_parts[common] == target_parts[common]
    {
        common += 1;
    }

    let ups = from_parts.len() - common;
    let mut out = String::new();
    for _ in 0..ups {
        out.push_str("../");
    }
    if ups == 0 {
        out.push_str("./");
    }
    out.push_str(&target_parts[common..].join("/"));
    out
}

fn normalize_repo_path(path: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for component in Path::new(path).components() {
        match component {
            Component::CurDir | Component::RootDir => {}
            Component::ParentDir => {
                let _ = parts.pop();
            }
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::Prefix(_) => {}
        }
    }
    parts.join("/")
}

fn encode_query_value(value: &str) -> String {
    use std::fmt::Write;

    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            _ => {
                let _ = write!(&mut encoded, "%{byte:02X}");
            }
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::{render, render_for_file, repo_relative_link_target, resolve_cover_url};
    use brain_domain::TargetConfig;

    fn test_cfg() -> TargetConfig {
        TargetConfig {
            org: "Dritara-Digital".into(),
            repo: "Brain".into(),
            branch: "main".into(),
        }
    }

    #[test]
    fn render_keeps_external_links() {
        let html = render("[site](https://example.com)");
        assert!(html.contains(r#"href="https://example.com""#));
    }

    #[test]
    fn render_rewrites_relative_markdown_links_to_knowledge_route() {
        let html = render_for_file(
            "[ADR](../adrs/001-git-centric-automation.md)",
            "concepts/foo.md",
            &test_cfg(),
        );
        assert!(html.contains(r#"href="/Dritara-Digital/Brain/main/knowledge?path=adrs%2F001-git-centric-automation.md""#));
    }

    #[test]
    fn render_rewrites_other_markdown_files_to_knowledge_route() {
        let html = render_for_file(
            "[Runbook](../templates/Runbook.md)",
            "concepts/foo.md",
            &test_cfg(),
        );
        assert!(html.contains(
            r#"href="/Dritara-Digital/Brain/main/knowledge?path=templates%2FRunbook.md""#
        ));
    }

    #[test]
    fn render_rewrites_images_to_github_raw() {
        let html = render_for_file(
            "![img](../screenshots/graph.png)",
            "concepts/foo.md",
            &test_cfg(),
        );
        assert!(html.contains(r#"src="https://raw.githubusercontent.com/Dritara-Digital/Brain/main/screenshots/graph.png""#));
    }

    #[test]
    fn render_routes_assets_images_through_proxy() {
        let html = render_for_file(
            "![img](/assets/2026/04/foo-abc.png)",
            "concepts/foo.md",
            &test_cfg(),
        );
        assert!(
            html.contains(r#"src="/Dritara-Digital/Brain/assets/2026/04/foo-abc.png""#),
            "got: {html}"
        );
    }

    // --- Security: XSS / content trust boundary ---------------------------

    #[test]
    fn render_escapes_script_tags() {
        let html = render("hello <script>alert(1)</script> world");
        assert!(!html.contains("<script>"), "got: {html}");
        assert!(html.contains("&lt;script&gt;"), "got: {html}");
    }

    #[test]
    fn render_escapes_iframe() {
        let html = render(r#"<iframe src="https://evil.example"></iframe>"#);
        assert!(!html.contains("<iframe"), "got: {html}");
    }

    #[test]
    fn render_strips_onerror_image_handler() {
        let html = render(r#"<img src=x onerror="alert(1)">"#);
        // The raw <img> tag is escaped to inert text — no live element/attribute
        // reaches the DOM. The literal string may survive, but only as `&lt;img…`.
        assert!(!html.contains("<img"), "live img tag survived: {html}");
        assert!(html.contains("&lt;img"), "expected escaped img: {html}");
    }

    #[test]
    fn render_drops_javascript_scheme_links() {
        // Link rewriting neutralizes `javascript:` in BOTH builds (no ammonia in
        // WASM), so the editor preview can't execute it via inner_html.
        let html = render("[click](javascript:alert(1))");
        assert!(
            !html.contains("javascript:alert"),
            "javascript: scheme should not survive, got: {html}"
        );
    }

    #[test]
    fn render_neutralizes_dangerous_schemes() {
        for payload in [
            "[x](javascript:alert(1))",
            "[x](JavaScript:alert(1))",
            "[x](vbscript:msgbox(1))",
            "[x](data:text/html,<script>alert(1)</script>)",
            "![x](data:image/svg+xml;base64,PHN2Zz4=)",
        ] {
            let html = render(payload);
            assert!(
                !html.to_lowercase().contains("javascript:")
                    && !html.to_lowercase().contains("vbscript:")
                    && !html.to_lowercase().contains("data:"),
                "dangerous scheme survived for {payload}: {html}"
            );
        }
    }

    #[test]
    fn render_keeps_safe_schemes() {
        // http(s)/mailto/tel must pass through untouched.
        assert!(render("[s](https://example.com)").contains(r#"href="https://example.com""#));
        assert!(render("[m](mailto:a@b.c)").contains(r#"href="mailto:a@b.c""#));
        assert!(render("[t](tel:+15551234)").contains(r#"href="tel:+15551234""#));
    }

    #[test]
    fn render_keeps_safe_formatting() {
        // Sanitization must not strip legitimate markdown-generated formatting.
        let html = render("# Title\n\n- [x] done\n- [ ] todo\n\n**bold** and `code`");
        assert!(html.contains("<h1>"), "got: {html}");
        assert!(html.contains("<strong>bold</strong>"), "got: {html}");
        assert!(html.contains("<code>code</code>"), "got: {html}");
        assert!(html.contains("<ul>"), "got: {html}");
    }

    #[test]
    fn render_comment_style_html_is_neutralized() {
        // GitHub comments are rendered through `render()` into the detail panel
        // via inner_html — the highest-risk external-provider surface.
        let html =
            render(r#"<a href="javascript:alert(document.cookie)">x</a><script>steal()</script>"#);
        // Raw HTML is escaped to text: no live <a>/<script> element, so the
        // javascript: href can never fire and the script never executes.
        assert!(!html.contains("<script"), "live script survived: {html}");
        assert!(!html.contains("<a "), "live anchor survived: {html}");
        assert!(
            html.contains("&lt;script&gt;"),
            "expected escaped script: {html}"
        );
    }

    #[test]
    fn repo_relative_link_target_points_from_markdown_file_to_asset() {
        assert_eq!(
            repo_relative_link_target(Some("concepts/foo.md"), "assets/2026/04/foo-abc.png"),
            "../assets/2026/04/foo-abc.png"
        );
        assert_eq!(
            repo_relative_link_target(
                Some("concepts/bozza-manifesto/foo.md"),
                "assets/2026/04/foo-abc.png"
            ),
            "../../assets/2026/04/foo-abc.png"
        );
        assert_eq!(
            repo_relative_link_target(Some("foo.md"), "assets/2026/04/foo-abc.png"),
            "./assets/2026/04/foo-abc.png"
        );
    }

    #[test]
    fn resolve_cover_url_routes_repo_assets_through_proxy() {
        // `assets/...` (rooted from `.` of the doc) should go through the
        // authenticated proxy, exactly like inline `![img](../assets/...)`.
        let url = resolve_cover_url("../assets/hero.png", "concepts/foo.md", &test_cfg());
        assert_eq!(
            url.as_deref(),
            Some("/Dritara-Digital/Brain/assets/hero.png")
        );
    }

    #[test]
    fn resolve_cover_url_keeps_absolute_https_untouched() {
        let url = resolve_cover_url(
            "https://example.com/hero.png",
            "concepts/foo.md",
            &test_cfg(),
        );
        assert_eq!(url.as_deref(), Some("https://example.com/hero.png"));
    }

    #[test]
    fn resolve_cover_url_falls_back_to_raw_for_non_assets_paths() {
        // A repo path that isn't under `assets/` falls back to the raw GitHub
        // host — same rule as inline images. Useful for screenshots checked
        // into other folders.
        let url = resolve_cover_url("../screenshots/x.png", "concepts/foo.md", &test_cfg());
        assert_eq!(
            url.as_deref(),
            Some("https://raw.githubusercontent.com/Dritara-Digital/Brain/main/screenshots/x.png")
        );
    }

    #[test]
    fn resolve_cover_url_drops_dangerous_schemes() {
        // `javascript:` / `data:` covers must yield None so the panel can omit
        // the hero block, not emit a placeholder `src="#"`.
        assert!(resolve_cover_url("javascript:alert(1)", "x.md", &test_cfg()).is_none());
        assert!(resolve_cover_url("data:image/svg+xml;base64,x", "x.md", &test_cfg()).is_none());
    }

    #[test]
    fn resolve_cover_url_drops_empty() {
        assert!(resolve_cover_url("", "x.md", &test_cfg()).is_none());
        assert!(resolve_cover_url("   ", "x.md", &test_cfg()).is_none());
    }

    #[test]
    fn render_preserves_mermaid_and_highlighting_classes() {
        // Test that mermaid blocks preserve their class="language-mermaid"
        let html = render("```mermaid\ngraph TD\n```");
        assert!(
            html.contains(r#"class="language-mermaid""#),
            "expected language-mermaid class to be preserved by ammonia: {html}"
        );

        // Test that standard code highlighting classes (like rust) are also preserved
        let html_rust = render("```rust\nlet x = 1;\n```");
        assert!(
            html_rust.contains(r#"class="language-rust""#),
            "expected language-rust class to be preserved by ammonia: {html_rust}"
        );
    }
}
