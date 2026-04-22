//! Parsing helpers: markdown file → internal `Parsed` intermediate.

use brain_domain::split_frontmatter;

/// A typed Brain doc parsed out of raw markdown. Internal to the graph build;
/// not exposed as a public API surface.
pub struct Parsed {
    pub rel: String,
    pub sha: String,
    pub title: String,
    pub summary: String,
    pub node_type: String,
    pub tags: Vec<String>,
    pub links: Vec<String>,
}

/// True if a path should be included in the Brain graph.
/// Skips hidden segments, `templates/`, README, AGENTS.
pub fn is_included_md(path: &str) -> bool {
    if !path.ends_with(".md") {
        return false;
    }
    for seg in path.split('/') {
        if seg.starts_with('.') || seg == "templates" {
            return false;
        }
    }
    let file = path.rsplit('/').next().unwrap_or(path);
    if file.eq_ignore_ascii_case("README.md") || file.eq_ignore_ascii_case("AGENTS.md") {
        return false;
    }
    true
}

pub fn parse_file(raw: &str, rel: &str, sha: &str) -> Option<Parsed> {
    let (front, body) = split_frontmatter(raw);
    if front.is_empty() {
        return None;
    }

    let mut node_type: Option<String> = None;
    let mut topic = String::new();
    let mut tags: Vec<String> = Vec::new();

    for line in front.lines() {
        let line = line.trim_end();
        if let Some(rest) = line.strip_prefix("type:") {
            let v = rest.trim().trim_matches('"');
            node_type = Some(v.to_string());
        } else if let Some(rest) = line.strip_prefix("topic:") {
            topic = rest.trim().trim_matches('"').to_string();
        } else if let Some(rest) = line.strip_prefix("tags:") {
            let v = rest.trim();
            if let Some(inner) = v.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                tags = inner
                    .split(',')
                    .map(|t| t.trim().trim_matches('"').trim_matches('\'').to_string())
                    .filter(|t| !t.is_empty())
                    .collect();
            }
        }
    }

    let node_type = node_type?;

    let title = if !topic.is_empty() {
        title_case(&topic)
    } else {
        first_heading(body).unwrap_or_else(|| {
            std::path::Path::new(rel)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| rel.to_string())
        })
    };

    let summary = extract_summary(body);
    let links = extract_links(body);

    Some(Parsed {
        rel: rel.to_string(),
        sha: sha.to_string(),
        title,
        summary,
        node_type,
        tags,
        links,
    })
}

fn first_heading(body: &str) -> Option<String> {
    for line in body.lines() {
        let l = line.trim_start();
        if let Some(rest) = l.strip_prefix("# ") {
            return Some(clean_heading(rest));
        }
    }
    None
}

fn clean_heading(h: &str) -> String {
    let mut s = h.trim().to_string();
    for prefix in ["Concept: ", "ADR: ", "Meeting: ", "ADR 001: ", "ADR 002: "] {
        if s.starts_with(prefix) {
            s = s.trim_start_matches(prefix).to_string();
        }
    }
    if s.len() > 60 {
        s.truncate(57);
        s.push('…');
    }
    s
}

fn title_case(s: &str) -> String {
    let s = s.replace(['-', '_'], " ");
    s.split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn extract_summary(body: &str) -> String {
    let mut in_summary = false;
    let mut buf = String::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            if in_summary {
                break;
            }
            if trimmed.to_lowercase().contains("summary")
                || trimmed.to_lowercase().contains("riepilogo")
            {
                in_summary = true;
                continue;
            }
        }
        if in_summary {
            if trimmed.is_empty() {
                if !buf.is_empty() {
                    break;
                }
                continue;
            }
            if trimmed.starts_with('>') || trimmed.starts_with('*') && trimmed.ends_with('*') {
                continue;
            }
            if !buf.is_empty() {
                buf.push(' ');
            }
            buf.push_str(trimmed);
        }
    }
    if buf.is_empty() {
        for line in body.lines() {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') || t.starts_with('>') {
                continue;
            }
            buf = t.to_string();
            break;
        }
    }
    let cleaned = strip_md(&buf);
    truncate(&cleaned, 180)
}

fn strip_md(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '['
            && let Some(close) = s[i..].find("](")
        {
            let text_end = i + close;
            out.push_str(&s[i + 1..text_end]);
            if let Some(paren) = s[text_end..].find(')') {
                let skip_to = text_end + paren + 1;
                while let Some(&(j, _)) = chars.peek() {
                    if j >= skip_to {
                        break;
                    }
                    chars.next();
                }
                continue;
            }
        }
        if c == '*' || c == '_' || c == '`' {
            continue;
        }
        out.push(c);
    }
    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn extract_links(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b']'
            && i + 1 < bytes.len()
            && bytes[i + 1] == b'('
            && let Some(end) = body[i + 2..].find(')')
        {
            let url = &body[i + 2..i + 2 + end];
            if url.ends_with(".md") && !url.starts_with("http") {
                let clean = url.split('#').next().unwrap_or(url).to_string();
                out.push(clean);
            }
            i = i + 2 + end + 1;
            continue;
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn include_rules() {
        assert!(is_included_md("concepts/Foo.md"));
        assert!(!is_included_md("README.md"));
        assert!(!is_included_md("AGENTS.md"));
        assert!(!is_included_md("templates/X.md"));
        assert!(!is_included_md(".hidden/X.md"));
        assert!(!is_included_md("concepts/Foo.txt"));
    }

    #[test]
    fn parse_minimal_concept() {
        let p = parse_file(
            "---\ntype: concept\ntopic: alpha-beta\n---\nbody",
            "concepts/AB.md",
            "sha1",
        )
        .unwrap();
        assert_eq!(p.title, "Alpha Beta");
        assert_eq!(p.node_type, "concept".to_string());
    }

    #[test]
    fn parse_extracts_md_links_excluding_external() {
        let raw = "---\ntype: concept\ntopic: X\n---\nsee [A](../a.md) and [ext](https://e.com)\n";
        let p = parse_file(raw, "concepts/X.md", "s").unwrap();
        assert_eq!(p.links, vec!["../a.md"]);
    }

    #[test]
    fn parse_rejects_missing_frontmatter() {
        assert!(parse_file("# just a heading", "x.md", "s").is_none());
    }

    #[test]
    fn parse_rejects_unknown_type() {
        let raw = "---\ntype: unknown\n---\n";
        assert!(parse_file(raw, "x.md", "s").is_none());
    }

    #[test]
    fn summary_prefers_summary_section() {
        let raw = "---\ntype: concept\ntopic: X\n---\n# Head\n\n## Summary\nHello world.\n\n## Other\nignore\n";
        let p = parse_file(raw, "x.md", "s").unwrap();
        assert_eq!(p.summary, "Hello world.");
    }
}
