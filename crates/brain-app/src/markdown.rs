//! Shared markdown helpers used by both SSR (rendering persisted files) and
//! the client-side hydrate build (live preview in the editor).

use brain_domain::{GithubClient, TargetConfig};
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
    out
}

fn rewrite_event<'a>(
    event: pulldown_cmark::Event<'a>,
    file_path: Option<&str>,
    cfg: Option<&TargetConfig>,
) -> pulldown_cmark::Event<'a> {
    use pulldown_cmark::{CowStr, Event, Tag};

    match event {
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
    if dest.is_empty() || dest.starts_with('#') || has_url_scheme(dest) {
        return dest.to_string();
    }

    if is_app_route(dest) {
        return dest.to_string();
    }

    let (path_part, fragment) = split_fragment(dest);
    let resolved = resolve_repo_path(file_path, path_part);

    if resolved.ends_with(".md") {
        let mut url = format!("/knowledge?path={}", encode_query_value(&resolved));
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

fn has_url_scheme(dest: &str) -> bool {
    dest.starts_with("http://")
        || dest.starts_with("https://")
        || dest.starts_with("mailto:")
        || dest.starts_with("tel:")
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
    use super::{render, render_for_file};
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
        assert!(html.contains(r#"href="/knowledge?path=adrs%2F001-git-centric-automation.md""#));
    }

    #[test]
    fn render_rewrites_other_markdown_files_to_knowledge_route() {
        let html = render_for_file(
            "[Runbook](../templates/Runbook.md)",
            "concepts/foo.md",
            &test_cfg(),
        );
        assert!(html.contains(r#"href="/knowledge?path=templates%2FRunbook.md""#));
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
}
