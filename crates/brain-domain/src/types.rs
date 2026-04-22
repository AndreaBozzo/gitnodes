use crate::frontmatter::split_frontmatter;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeType {
    Concept,
    Decision,
    Meeting,
    PostMortem,
    Preventivo,
    Runbook,
    Tag,
}

impl NodeType {
    pub const ALL: [NodeType; 7] = [
        NodeType::Concept,
        NodeType::Decision,
        NodeType::Meeting,
        NodeType::PostMortem,
        NodeType::Preventivo,
        NodeType::Runbook,
        NodeType::Tag,
    ];

    /// Types a user can create via the editor (excludes Tag, which is virtual).
    pub const CREATABLE: [NodeType; 6] = [
        NodeType::Concept,
        NodeType::Decision,
        NodeType::Meeting,
        NodeType::PostMortem,
        NodeType::Preventivo,
        NodeType::Runbook,
    ];

    pub fn label(self) -> &'static str {
        match self {
            NodeType::Concept => "Concept",
            NodeType::Decision => "ADR",
            NodeType::Meeting => "Meeting",
            NodeType::PostMortem => "Post-mortem",
            NodeType::Preventivo => "Preventivo",
            NodeType::Runbook => "Runbook",
            NodeType::Tag => "Tag",
        }
    }

    pub fn accent(self) -> &'static str {
        match self {
            NodeType::Concept => "#2dd4bf",
            NodeType::Decision => "#f59e0b",
            NodeType::Meeting => "#a78bfa",
            NodeType::PostMortem => "#f87171",
            NodeType::Preventivo => "#38bdf8",
            NodeType::Runbook => "#4ade80",
            NodeType::Tag => "#64748b",
        }
    }

    /// CSS custom-property reference for the accent colour.
    /// Use in HTML `style` attributes; for SVG fills prefer `accent()`.
    pub fn accent_var(self) -> &'static str {
        match self {
            NodeType::Concept => "var(--accent-concept)",
            NodeType::Decision => "var(--accent-decision)",
            NodeType::Meeting => "var(--accent-meeting)",
            NodeType::PostMortem => "var(--accent-postmortem)",
            NodeType::Preventivo => "var(--accent-preventivo)",
            NodeType::Runbook => "var(--accent-runbook)",
            NodeType::Tag => "var(--accent-tag)",
        }
    }

    /// Returns the Brain repo directory for this type.
    pub fn directory(self) -> &'static str {
        match self {
            NodeType::Concept => "concepts",
            NodeType::Decision => "adrs",
            NodeType::Meeting => "meetings",
            NodeType::PostMortem => "post-mortems",
            NodeType::Preventivo => "preventivi",
            NodeType::Runbook => "runbooks",
            NodeType::Tag => "",
        }
    }

    /// Reverse of `directory()`: maps a canonical folder name back to the
    /// type that owns it. Returns `None` for unknown folders (custom paths).
    pub fn from_directory(dir: &str) -> Option<Self> {
        match dir.trim_matches('/') {
            "concepts" => Some(NodeType::Concept),
            "adrs" => Some(NodeType::Decision),
            "meetings" => Some(NodeType::Meeting),
            "post-mortems" => Some(NodeType::PostMortem),
            "preventivi" => Some(NodeType::Preventivo),
            "runbooks" => Some(NodeType::Runbook),
            _ => None,
        }
    }

    /// Returns the Brain template frontmatter type value.
    pub fn frontmatter_type(self) -> &'static str {
        match self {
            NodeType::Concept => "concept",
            NodeType::Decision => "adr",
            NodeType::Meeting => "meeting",
            NodeType::PostMortem => "post-mortem",
            NodeType::Preventivo => "preventivo",
            NodeType::Runbook => "runbook",
            NodeType::Tag => "",
        }
    }

    /// Filename under `templates/` in the Brain repo. None = no template.
    pub fn template_filename(self) -> Option<&'static str> {
        match self {
            NodeType::Concept => Some("ConceptNote.md"),
            NodeType::Decision => Some("ADR.md"),
            NodeType::PostMortem => Some("PostMortem.md"),
            NodeType::Preventivo => Some("Preventivo.md"),
            NodeType::Runbook => Some("Runbook.md"),
            NodeType::Meeting | NodeType::Tag => None,
        }
    }
}

impl fmt::Display for NodeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub id: u32,
    pub title: String,
    pub summary: String,
    pub node_type: NodeType,
    pub tags: Vec<String>,
    pub x: f32,
    pub y: f32,
    /// Relative path in the Brain repo (e.g. "concepts/Foo.md").
    #[serde(default)]
    pub path: String,
    /// GitHub file SHA for optimistic concurrency on updates.
    #[serde(default)]
    pub sha: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Edge {
    pub from: u32,
    pub to: u32,
}

/// Snapshot of an existing doc's structured fields, used to prefill the editor on edit.
#[derive(Clone, Debug, Default)]
pub struct EditPrefill {
    pub path: String,
    pub sha: String,
    pub node_type: Option<NodeType>,
    pub title: String,
    pub author: String,
    pub tags: Vec<String>,
    /// Full body (everything after the frontmatter), preserved verbatim.
    pub body: String,
    pub related: Vec<String>,
    /// Full parsed frontmatter, preserved so `save_brain_file` can merge
    /// form-controlled fields without wiping custom keys (status, severity,
    /// cliente, etc.). Empty map if the file has no frontmatter.
    pub frontmatter: BTreeMap<String, serde_yaml::Value>,
    /// True when the source file had a frontmatter block that failed YAML
    /// parsing. The editor must surface this and prevent saves to avoid
    /// silently rewriting the file from defaults and dropping the original
    /// keys.
    pub frontmatter_malformed: bool,
}

/// What the editor panel should do when open.
#[derive(Clone, Debug, Default)]
pub enum EditMode {
    #[default]
    Closed,
    New,
    Edit(EditPrefill),
}

impl EditPrefill {
    /// Parse a raw markdown file (with YAML frontmatter) from the Brain repo
    /// into the structured fields the editor expects. Body is preserved verbatim.
    pub fn from_raw(path: &str, sha: &str, raw: &str) -> Self {
        let (front, body) = split_frontmatter(raw);
        let mut out = EditPrefill {
            path: path.to_string(),
            sha: sha.to_string(),
            body: body.to_string(),
            ..Default::default()
        };

        if !front.trim().is_empty() {
            match serde_yaml::from_str::<BTreeMap<String, serde_yaml::Value>>(front) {
                Ok(map) => out.frontmatter = map,
                Err(_) => {
                    out.frontmatter_malformed = true;
                }
            }
        }

        if let Some(v) = out.frontmatter.get("type").and_then(|v| v.as_str()) {
            out.node_type = match v {
                "concept" => Some(NodeType::Concept),
                "adr" => Some(NodeType::Decision),
                "meeting" => Some(NodeType::Meeting),
                "post-mortem" => Some(NodeType::PostMortem),
                "preventivo" => Some(NodeType::Preventivo),
                "runbook" => Some(NodeType::Runbook),
                _ => None,
            };
        }
        // Title lives under `topic:` for concepts, `progetto:` for preventivi.
        // Fall back to the other key then to the H1 heading below.
        let title_key = matches!(out.node_type, Some(NodeType::Preventivo))
            .then_some("progetto")
            .unwrap_or("topic");
        if let Some(v) = out.frontmatter.get(title_key).and_then(|v| v.as_str()) {
            out.title = v.to_string();
        }
        if out.title.is_empty() {
            // Cross-fallback so a typo or legacy file doesn't drop the title.
            let alt = if title_key == "topic" {
                "progetto"
            } else {
                "topic"
            };
            if let Some(v) = out.frontmatter.get(alt).and_then(|v| v.as_str()) {
                out.title = v.to_string();
            }
        }
        if let Some(v) = out.frontmatter.get("author").and_then(|v| v.as_str()) {
            out.author = v.to_string();
        }
        if let Some(seq) = out.frontmatter.get("tags").and_then(|v| v.as_sequence()) {
            out.tags = seq
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .filter(|s| !s.is_empty())
                .collect();
        }

        // If no `topic:` field, derive title from the first heading.
        if out.title.is_empty() {
            for line in body.lines() {
                let l = line.trim_start();
                if let Some(rest) = l.strip_prefix("# ") {
                    let mut t = rest.trim().to_string();
                    for prefix in ["Concept: ", "ADR: ", "Meeting: "] {
                        if t.starts_with(prefix) {
                            t = t.trim_start_matches(prefix).to_string();
                        }
                    }
                    out.title = t;
                    break;
                }
            }
        }

        // Extract related links under "## Related / See also".
        let mut in_related = false;
        for line in body.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("## ") {
                in_related = trimmed.to_lowercase().contains("related")
                    || trimmed.to_lowercase().contains("see also");
                continue;
            }
            if in_related
                && let Some(rest) = trimmed.strip_prefix("- ")
                && let Some(open) = rest.find("](")
                && let Some(close) = rest[open + 2..].find(')')
            {
                let url = &rest[open + 2..open + 2 + close];
                let cleaned = url
                    .trim_start_matches("../")
                    .split('#')
                    .next()
                    .unwrap_or(url);
                if cleaned.ends_with(".md") && !cleaned.starts_with("http") {
                    out.related.push(cleaned.to_string());
                }
            }
        }

        out
    }
}

/// Payload sent from the editor form to create/update a Brain file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrainFilePayload {
    pub node_type: NodeType,
    pub title: String,
    pub author: String,
    pub tags: Vec<String>,
    pub body: String,
    /// Related file paths chosen via forced-linking.
    pub related: Vec<String>,
    pub folder: Option<String>,
    /// For updates: the file path and sha.
    pub path: Option<String>,
    pub sha: Option<String>,
    /// Optional user-supplied commit message. Empty/None falls back to the
    /// auto-generated "Update/Create X via Brain UI" message.
    #[serde(default)]
    pub commit_message: Option<String>,
    /// Frontmatter parsed from the original file on update. `save_brain_file`
    /// merges form-controlled fields on top of this map so custom keys
    /// (status, severity, cliente, etc.) survive the round-trip. `None` on
    /// create; old clients without the field still deserialize via serde
    /// default.
    #[serde(default)]
    pub preserved_frontmatter: Option<BTreeMap<String, serde_yaml::Value>>,
    /// True when the source file's frontmatter failed to parse. Blocks save
    /// server-side so we don't silently rewrite the file from defaults.
    #[serde(default)]
    pub frontmatter_malformed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_type_creatable_excludes_tag() {
        assert!(!NodeType::CREATABLE.contains(&NodeType::Tag));
        assert_eq!(NodeType::CREATABLE.len(), 6);
    }

    #[test]
    fn node_type_directory_roundtrip_distinct() {
        let dirs: Vec<&str> = NodeType::CREATABLE.iter().map(|t| t.directory()).collect();
        let mut sorted = dirs.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(dirs.len(), sorted.len(), "directories must be unique");
    }

    #[test]
    fn prefill_parses_topic_and_tags() {
        let raw = "---\ntype: adr\ntopic: Foo Bar\ntags: [\"a\", b, 'c']\n---\nbody\n";
        let p = EditPrefill::from_raw("adrs/Foo.md", "abc", raw);
        assert_eq!(p.title, "Foo Bar");
        assert_eq!(p.node_type, Some(NodeType::Decision));
        assert_eq!(p.tags, vec!["a", "b", "c"]);
        assert_eq!(p.body, "body\n");
    }

    #[test]
    fn prefill_falls_back_to_h1_title() {
        let raw = "---\ntype: concept\n---\n# Concept: Alpha\n\ncontent";
        let p = EditPrefill::from_raw("concepts/Alpha.md", "sha", raw);
        assert_eq!(p.title, "Alpha");
    }

    #[test]
    fn prefill_extracts_related_links() {
        let raw = "---\ntype: concept\n---\n# X\n\n## Related\n- [Foo](../concepts/Foo.md)\n- [ext](https://example.com)\n";
        let p = EditPrefill::from_raw("concepts/X.md", "s", raw);
        assert_eq!(p.related, vec!["concepts/Foo.md"]);
    }

    #[test]
    fn prefill_preserves_custom_frontmatter_fields() {
        // Caveat #5: a doc with custom fields (status: accepted on a non-draft
        // ADR, severity on a post-mortem) must round-trip the full map so
        // save_brain_file can merge instead of regenerating.
        let raw = "---\ntype: adr\nstatus: accepted\ndate: 2026-03-01\nauthor: alice\ntags: [\"x\"]\n---\nbody\n";
        let p = EditPrefill::from_raw("adrs/F.md", "sha", raw);
        assert_eq!(p.node_type, Some(NodeType::Decision));
        assert_eq!(
            p.frontmatter.get("status").and_then(|v| v.as_str()),
            Some("accepted")
        );
        assert_eq!(
            p.frontmatter.get("date").and_then(|v| v.as_str()),
            Some("2026-03-01")
        );
    }

    #[test]
    fn prefill_malformed_yaml_flags_without_panicking() {
        // Malformed YAML frontmatter must not panic, must be flagged so the
        // server can refuse to silently rewrite the file from defaults even
        // if the UI block is bypassed.
        let raw = "---\ntype: concept\n: not valid :\n---\nbody";
        let p = EditPrefill::from_raw("concepts/X.md", "s", raw);
        assert!(p.frontmatter.is_empty());
        assert!(p.frontmatter_malformed);
        assert_eq!(p.body, "body");

        let payload = BrainFilePayload {
            node_type: NodeType::Concept,
            title: "X".into(),
            author: "alice".into(),
            tags: vec![],
            body: p.body.clone(),
            related: p.related.clone(),
            folder: None,
            path: Some(p.path.clone()),
            sha: Some(p.sha.clone()),
            commit_message: None,
            preserved_frontmatter: if p.frontmatter.is_empty() {
                None
            } else {
                Some(p.frontmatter.clone())
            },
            frontmatter_malformed: p.frontmatter_malformed,
        };
        assert!(payload.frontmatter_malformed);
    }

    #[test]
    fn prefill_preventivo_title_from_progetto() {
        // Preventivo stores its title under `progetto:`, not `topic:`. Without
        // this fallback the editor would open with an empty title and a save
        // would blank out progetto.
        let raw = "---\ntype: preventivo\nprogetto: \"Rewrite search\"\nstatus: draft\n---\n";
        let p = EditPrefill::from_raw("preventivi/Foo.md", "s", raw);
        assert_eq!(p.node_type, Some(NodeType::Preventivo));
        assert_eq!(p.title, "Rewrite search");
    }
}
