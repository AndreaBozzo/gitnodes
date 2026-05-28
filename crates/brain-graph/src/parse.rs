//! Parsing helpers: markdown file → internal `Parsed` intermediate.

use brain_domain::{BrainConfig, EdgeKind, split_frontmatter};
use serde_yaml::Value;

/// A typed Brain doc parsed out of raw markdown. Internal to the graph build;
/// not exposed as a public API surface.
pub struct Parsed {
    pub rel: String,
    pub sha: String,
    pub title: String,
    pub summary: String,
    pub node_type: String,
    pub tags: Vec<String>,
    pub links: Vec<ParsedLink>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedLink {
    pub target: String,
    pub kind: EdgeKind,
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

pub fn parse_file(raw: &str, rel: &str, sha: &str, config: &BrainConfig) -> Option<Parsed> {
    let (front, body) = split_frontmatter(raw);
    if front.is_empty() {
        return None;
    }

    let frontmatter = serde_yaml::from_str::<Value>(front).ok()?;
    let node_type = yaml_string(&frontmatter, "type")?;
    let tags = yaml_string_sequence(&frontmatter, "tags");

    let title = config
        .lookup(&node_type)
        .and_then(|spec| spec.title_key.as_deref())
        .and_then(|key| {
            yaml_string(&frontmatter, key).map(|value| {
                if key == "topic" {
                    title_case(value.as_str())
                } else {
                    value
                }
            })
        })
        .or_else(|| yaml_string(&frontmatter, "topic").map(|topic| title_case(topic.as_str())))
        .unwrap_or_else(|| {
            first_heading(body).unwrap_or_else(|| {
                std::path::Path::new(rel)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| rel.to_string())
            })
        });

    let summary = extract_summary(body);
    let mut links = extract_links(body);
    links.extend(extract_frontmatter_links(&frontmatter, &node_type, config));

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

fn yaml_string(root: &Value, key: &str) -> Option<String> {
    root.as_mapping()?
        .get(Value::String(key.to_string()))?
        .as_str()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
}

fn yaml_string_sequence(root: &Value, key: &str) -> Vec<String> {
    root.as_mapping()
        .and_then(|map| map.get(Value::String(key.to_string())))
        .and_then(Value::as_sequence)
        .map(|seq| {
            seq.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|tag| !tag.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
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

fn extract_links(body: &str) -> Vec<ParsedLink> {
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
                out.push(ParsedLink {
                    target: clean,
                    kind: EdgeKind::Body,
                });
            }
            i = i + 2 + end + 1;
            continue;
        }
        i += 1;
    }
    out
}

fn extract_frontmatter_links(
    frontmatter: &Value,
    node_type: &str,
    config: &BrainConfig,
) -> Vec<ParsedLink> {
    let Some(spec) = config.lookup(node_type) else {
        return Vec::new();
    };
    let Some(mapping) = frontmatter.as_mapping() else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for (field, target_type) in &spec.link_fields {
        let Some(target_spec) = config.lookup(target_type) else {
            continue;
        };
        let Some(value) = mapping.get(Value::String(field.clone())) else {
            continue;
        };
        for slug in yaml_link_values(value) {
            if let Some(target) = frontmatter_slug_to_path(&slug, target_spec.directory.as_str()) {
                out.push(ParsedLink {
                    target,
                    kind: EdgeKind::Frontmatter(field.clone()),
                });
            }
        }
    }
    out
}

fn yaml_link_values(value: &Value) -> Vec<String> {
    if let Some(single) = value.as_str().map(str::trim).filter(|s| !s.is_empty()) {
        return vec![single.to_string()];
    }
    value
        .as_sequence()
        .map(|seq| {
            seq.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn frontmatter_slug_to_path(slug: &str, directory: &str) -> Option<String> {
    let slug = slug.trim().trim_start_matches('/');
    let directory = directory.trim_matches('/');
    if slug.is_empty() || directory.is_empty() {
        return None;
    }
    if slug.ends_with(".md") {
        Some(format!("{directory}/{slug}"))
    } else {
        Some(format!("{directory}/{slug}.md"))
    }
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
            &BrainConfig::default(),
        )
        .unwrap();
        assert_eq!(p.title, "Alpha Beta");
        assert_eq!(p.node_type, "concept".to_string());
    }

    #[test]
    fn parse_prefers_configured_title_key_over_heading() {
        let mut config = BrainConfig::default();
        let concept = config
            .node_types
            .iter_mut()
            .find(|spec| spec.name == "concept")
            .unwrap();
        concept.title_key = Some("name".to_string());

        let p = parse_file(
            "---\ntype: concept\nname: Clean Label\n---\n# Concept: Noisy Heading\nbody",
            "concepts/clean.md",
            "sha1",
            &config,
        )
        .unwrap();

        assert_eq!(p.title, "Clean Label");
    }

    #[test]
    fn parse_extracts_block_sequence_tags() {
        let raw = "---\ntype: task\ntopic: Brain_UI Development\ntags:\n- Brain\n- brain-ui\n- rustlang\n---\nbody";
        let p = parse_file(
            raw,
            "tasks/BrainUI-Development-.md",
            "sha",
            &BrainConfig::default(),
        )
        .unwrap();
        assert_eq!(p.tags, vec!["Brain", "brain-ui", "rustlang"]);
    }

    #[test]
    fn parse_extracts_inline_sequence_tags() {
        let raw = "---\ntype: runbook\ntags: [\"Brain\", \"brain-ui\", workflow]\n---\nbody";
        let p = parse_file(raw, "runbooks/usage.md", "sha", &BrainConfig::default()).unwrap();
        assert_eq!(p.tags, vec!["Brain", "brain-ui", "workflow"]);
    }

    #[test]
    fn parse_extracts_md_links_excluding_external() {
        let raw = "---\ntype: concept\ntopic: X\n---\nsee [A](../a.md) and [ext](https://e.com)\n";
        let p = parse_file(raw, "concepts/X.md", "s", &BrainConfig::default()).unwrap();
        assert_eq!(p.links[0].target, "../a.md");
        assert_eq!(p.links[0].kind, EdgeKind::Body);
    }

    #[test]
    fn parse_rejects_missing_frontmatter() {
        assert!(parse_file("# just a heading", "x.md", "s", &BrainConfig::default()).is_none());
    }

    #[test]
    fn parse_preserves_unknown_type() {
        let raw = "---\ntype: unknown\n---\n";
        let parsed = parse_file(raw, "x.md", "s", &BrainConfig::default())
            .expect("unknown types now round-trip");
        assert_eq!(parsed.node_type, "unknown");
    }

    #[test]
    fn summary_prefers_summary_section() {
        let raw = "---\ntype: concept\ntopic: X\n---\n# Head\n\n## Summary\nHello world.\n\n## Other\nignore\n";
        let p = parse_file(raw, "x.md", "s", &BrainConfig::default()).unwrap();
        assert_eq!(p.summary, "Hello world.");
    }

    #[test]
    fn parse_extracts_configured_frontmatter_links() {
        let mut config = BrainConfig::default();
        let concept = config
            .node_types
            .iter_mut()
            .find(|spec| spec.name == "concept")
            .unwrap();
        concept
            .link_fields
            .insert("trainer".to_string(), "meeting".to_string());

        let raw = "---\ntype: concept\ntopic: X\ntrainer: ash-ketchum\n---\n";
        let p = parse_file(raw, "concepts/X.md", "s", &config).unwrap();

        assert_eq!(p.links.len(), 1);
        assert_eq!(p.links[0].target, "meetings/ash-ketchum.md");
        assert_eq!(
            p.links[0].kind,
            EdgeKind::Frontmatter("trainer".to_string())
        );
    }

    #[test]
    fn frontmatter_nested_slug_stays_under_target_directory() {
        assert_eq!(
            frontmatter_slug_to_path("kanto/ash.md", "trainers").as_deref(),
            Some("trainers/kanto/ash.md")
        );
        assert_eq!(
            frontmatter_slug_to_path("/kanto/ash", "trainers").as_deref(),
            Some("trainers/kanto/ash.md")
        );
    }
}
