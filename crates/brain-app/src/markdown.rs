//! Shared markdown helpers used by both SSR (rendering persisted files) and
//! the client-side hydrate build (live preview in the editor).

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
pub fn render(body: &str) -> String {
    use pulldown_cmark::{Options, Parser, html};
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_SMART_PUNCTUATION);
    let parser = Parser::new_ext(body, opts);
    let mut out = String::with_capacity(body.len() + body.len() / 4);
    html::push_html(&mut out, parser);
    out
}
